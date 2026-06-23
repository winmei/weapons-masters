# Stress Test Arquitetural — MMORPG Tab-Target 2026

---

## Categoria 1: Gargalos de Performance

---

### 🔴 BLOQUEADOR — P1: Redis como State Store do Game Loop

**A falha:** Você definiu Redis para guardar o "estado do mundo em memória (baixa latência)". Isso implica que o World Server faz chamadas de rede ao Redis **dentro do loop de simulação** (tick) para ler/escrever posições, HP, buffs e cooldowns.

Mesmo na melhor rede local, uma chamada Redis ida+volta leva **~0.1–0.3ms**. Parece pouco, mas no seu cenário de 1v5 com esquiva ativa, você precisa de **tick rate mínimo de 30Hz** (33ms por tick) — idealmente 60Hz (16ms). Se dentro de um único tick o servidor precisa consultar o Redis para 100 jogadores, calcular LoS, resolver colisões e gravar o resultado de volta:

```
100 players × ~4 leituras Redis por player (pos, hp, buffs, cooldowns) = 400 chamadas
400 × 0.15ms = 60ms → estoura o budget de 16ms (60Hz) e até o de 33ms (30Hz)
```

Pipeline e batching do Redis reduzem isso para ~5-8ms, mas ainda consome 30-50% do seu tick budget só em I/O de rede, sem contar o processamento real.

**Correção:**
O estado autoritativo do mundo deve viver **na memória do processo do World Server** (uma struct/hashmap in-process). O Redis serve exclusivamente para:
- Cache de dados de outros serviços (social, ranking, sessões)
- Pub/Sub entre World Servers (chat cross-map, notificações globais)
- Estado de reconexão (se o World Server crashar, o player reconecta e o Redis tem um snapshot recente para reconstruir)

```
Antes:  [World Server] --rede--> [Redis] --rede--> [World Server]  (0.15ms × N chamadas)
Depois: [World Server.state HashMap] → acesso direto em memória   (~50 nanossegundos)
```

**Trade-off:** Isso torna o World Server stateful. Você perde a capacidade de escalar horizontalmente adicionando réplicas stateless. A solução é o **Spatial Partitioning** — cada World Server "possui" uma região do mapa, e a escalabilidade é particionamento de espaço, não de réplicas.

---

### 🔴 BLOQUEADOR — P2: LoS e Colisão a O(N²) na Arena de 100 Jogadores

**A falha:** O combate com esquiva ativa e "strafing" exige que o servidor valide **Line of Sight (LoS)** e **hitbox de colisão** para cada habilidade usada. Sem estrutura espacial, verificar "quem está no campo de visão de quem" e "essa esquiva evitou o projétil" é N² por tick:

```
100 players × 100 alvos potenciais = 10.000 verificações de LoS por tick
A 60Hz = 600.000 raycasts por segundo
```

**Correção — duas camadas:**

1. **Broad Phase (Grid Espacial / Spatial Hash):** Divida a arena em células de, por exemplo, 20×20 unidades. Cada jogador pertence a uma célula. Verificações de LoS e colisão só acontecem entre entidades na **mesma célula ou células adjacentes**. Custo cai de O(N²) para O(N×K), onde K ≈ 8-15 vizinhos por célula.

2. **Narrow Phase (Raycasts simplificados):** Para Tab-Target, você não precisa de raycasts 3D completos. Projete o LoS em 2D (planta baixa / top-down) usando **segmentos de linha vs. obstáculos retangulares** (AABB). Isso é ordens de magnitude mais rápido que raycasts em um motor de física 3D.

```go
// Exemplo em Go: spatial hash O(1) lookup
type SpatialGrid struct {
    cellSize float64
    cells    map[CellKey][]EntityID  // lookup O(1) por célula
}

func (g *SpatialGrid) GetNeighbors(pos Vec2) []EntityID {
    key := CellKey{int(pos.X / g.cellSize), int(pos.Y / g.cellSize)}
    var result []EntityID
    for dx := -1; dx <= 1; dx++ {
        for dy := -1; dy <= 1; dy++ {
            result = append(result, g.cells[CellKey{key.X + dx, key.Y + dy}]...)
        }
    }
    return result // tipicamente 8-15 entidades, não 100
}
```

---

### ⚠️ MELHORIA — P3: Snapshot Bandwidth em Arenas Lotadas

**A falha:** Snapshot interpolation envia o estado completo do mundo visível para cada cliente a cada tick. Com 100 jogadores visíveis, cada snapshot contém ~100 entidades × ~60 bytes (pos + rot + HP + state + buffs) = **6KB por snapshot**. A 30Hz para 100 clientes:

```
6KB × 30Hz × 100 clients = 18 MB/s de upload do servidor
```

Isso é sustentável em cloud, mas comprime a margem.

**Correção — Delta Compression + Interest Management:**
- **Delta snapshots:** Envie apenas o que mudou desde o último snapshot confirmado pelo cliente. Jogadores parados ou em idle consomem zero bandwidth.
- **Area of Interest (AOI):** O jogador só recebe updates de entidades dentro de um raio relevante. Combinado com o Spatial Hash do P2, isso é trivial de implementar — envie apenas entidades das células vizinhas.

---

## Categoria 2: Vulnerabilidades de Segurança (Anti-Cheat)

---

### 🔴 BLOQUEADOR — S1: Peeker's Advantage Amplificado pelo Client Prediction

**A falha:** No seu modelo, o cliente aplica prediction local e envia intents ao servidor. O servidor processa e confirma. Em um cenário de esquiva ativa:

1. Player A (latência 20ms) usa uma skill no Player B (latência 120ms)
2. No servidor, a skill acerta B na posição X
3. Mas na tela de B, ele já se esquivou — só que essa esquiva ainda não chegou ao servidor (120ms de delay)

B percebe que foi acertado **depois de ter se esquivado** na tela dele. Isso é o **peeker's advantage inverso** — quem tem mais lag é punido desproporcionalmente em combate de precisão.

**Correção — Lag Compensation com Server-Side Rewind:**

O servidor mantém um buffer de **200ms de histórico de posições** para cada jogador. Quando Player A dispara uma skill, o servidor "rebobina" o mundo para `tempo_atual - latência_de_A` e verifica o hit na posição que B ocupava naquele instante:

```go
const RewindBufferDuration = 200 * time.Millisecond

type PositionHistory struct {
    entries []TimestampedPos // ring buffer de ~12 entries a 60Hz
}

func (s *CombatService) ProcessSkill(attacker, target EntityID, skillTime time.Time) HitResult {
    // Rebobina posição do alvo para o momento em que o atacante viu a cena
    attackerLatency := s.sessions[attacker].RTT / 2
    rewindTime := skillTime.Add(-attackerLatency)
    
    // Clamp: nunca rebobina mais que 200ms (impede abuso com lag artificial)
    if time.Since(rewindTime) > RewindBufferDuration {
        rewindTime = time.Now().Add(-RewindBufferDuration)
    }
    
    targetPosAtRewind := s.positionHistory[target].SampleAt(rewindTime)
    return s.checkHit(attacker, targetPosAtRewind, skill)
}
```

**Trade-off:** Isso significa que jogadores com lag muito alto (~150ms+) terão uma vantagem leve — eles podem ser acertados em posições "do passado" onde já não estão. Por isso o clamp de 200ms é crucial: limita o rewind máximo e nivela o campo.

---

### 🔴 BLOQUEADOR — S2: Falsificação de Input de Esquiva (Dodge Spoofing)

**A falha:** Se o cliente envia intents como `{action: "dodge", direction: "left", timestamp: T}`, um cheat pode:
- Enviar o dodge **retroativamente** com timestamp no passado, como se tivesse esquivado antes do hit chegar
- Inundar o servidor com dodge inputs (dodge-spam) para ter invencibilidade constante

**Correção — 3 defesas combinadas:**

1. **Rejeição de timestamps do passado:** O servidor ignora qualquer input com timestamp mais de `RTT/2 + 10ms` no passado. O cliente não controla "quando" a ação aconteceu — ele envia e o servidor carimba o momento de chegada.

2. **Cooldown autoritativo de dodge:** O servidor mantém `last_dodge_time` por jogador. Novo dodge só é aceito se `time.Now() - last_dodge_time > DODGE_COOLDOWN`. Não importa quantos inputs o cliente envie.

3. **Janela de invulnerabilidade finita:** Dodge concede i-frames (invulnerabilidade) por exatamente X ms no servidor. Fora dessa janela, hits são registrados normalmente.

---

### ⚠️ MELHORIA — S3: Flood de Input via WebRTC (Input Flooding DDoS)

**A falha:** WebRTC Data Channels não têm rate limiting nativo. Um cliente hackeado pode enviar 10.000 inputs por segundo via UDP, sobrecarregando o processamento do World Server.

**Correção:** Rate limiter **no Gateway**, antes de repassar ao World Server:
- Máximo de **30 inputs por segundo por jogador** (suficiente para 60Hz com margem)
- Inputs excedentes são descartados silenciosamente
- Se persistir por >5 segundos → desconexão automática + flag para análise

---

## Categoria 3: Questões de Design Arquitetural

---

### 🔴 BLOQUEADOR — D1: Controller/Service/Repository é um Padrão Web, Não de Game Server

**A falha:** Essa separação (Controller = rede, Service = lógica, Repository = estado) funciona perfeitamente para APIs REST/gRPC com request-response. Mas um game server opera em **loop contínuo** (tick-based), não em resposta a requests individuais.

O problema surge quando sua "Service" layer precisa, em um único tick, processar 100 intents de jogadores, resolver física, IA, colisões e montar snapshots — tudo em 16ms. Se a Service chama o Repository (Redis via rede) para cada operação, você tem o gargalo do P1. Se o Repository é in-memory, ele não é mais um Repository no sentido arquitetural — é apenas o estado do game loop.

**Correção — Arquitetura ECS (Entity-Component-System) para o Game Loop:**

Mantenha Controller/Service/Repository para os **microsserviços de apoio** (Auth, Social, Economy — que são request-response). Mas para o **World Server**, adote ECS:

```
┌──────────────────────────────────────────────────┐
│ World Server (loop contínuo a 30-60Hz)           │
│                                                  │
│  ┌─────────┐  ┌────────────┐  ┌──────────────┐  │
│  │ Systems  │→ │ Components │←→│   World      │  │
│  │(lógica)  │  │ (dados)    │  │  (entidades) │  │
│  └─────────┘  └────────────┘  └──────────────┘  │
│     ↑ inputs                  snapshots ↓        │
│  ┌──────────────┐        ┌───────────────────┐   │
│  │ NetworkInput │        │ SnapshotSerializer │   │
│  │ (Gateway)    │        │ (p/ Gateway)       │   │
│  └──────────────┘        └───────────────────┘   │
└──────────────────────────────────────────────────┘
```

- **Entities:** ID numérico (Player 42, Monster 107)
- **Components:** Structs de dados puros (Position, Health, CombatState, Buffs)
- **Systems:** Funções puras que operam sobre componentes (MovementSystem, CombatSystem, BuffSystem)

Isso te dá: dados contíguos em memória (cache-friendly), sistemas desacoplados, e funções puras (perfeitamente testáveis — veja T1).

---

### ⚠️ MELHORIA — D2: Go vs. C# vs. Node.js — Escolha Errada Paralisa o Projeto

**A falha:** Você não decidiu a linguagem do servidor. Isso não é um detalhe — é uma decisão que afeta tudo: performance do tick loop, disponibilidade de bibliotecas de física, ecossistema de ECS e modelo de concorrência.

**Análise comparativa para seu caso específico:**

| Critério | **Go** | **C#** | **Node.js** |
|:---|:---|:---|:---|
| Tick loop a 60Hz com 100 entidades | ✅ Excelente (goroutines, zero GC pressure com pools) | ✅ Bom (porém GC pauses de ~2-5ms no .NET) | ❌ Single-thread, GC unpredictable |
| ECS maduro | ⚠️ Limitado (precisa criar do zero) | ✅ Unity DOTS / Arch ECS | ❌ Nenhum battle-tested |
| Libs de física 2D server-side | ⚠️ Poucas (box2d-go immaturo) | ✅ Muitas (Box2D bindings, BepuPhysics) | ❌ Nenhuma performática |
| WebTransport/WebRTC server | ✅ pion/webrtc, quic-go | ⚠️ Menos maduro | ⚠️ Libs existem mas perf é problema |
| Deploy em container Linux | ✅ Binary estático de ~10MB | ✅ Mas runtime pesado (~200MB) | ✅ Mas runtime pesado |

**Recomendação:**
- **Go** se você quer controle total sobre alocações, GC mínimo e performance de rede raw. Mas terá que construir mais infra do zero (ECS, spatial hash, physics).
- **C#** se o jogo usa Godot com C# no cliente e você quer compartilhar modelos/DTOs entre client e server. Ecossistema de game server mais maduro.

**Node.js é eliminado** para um game loop autoritativo de precisão. O event loop single-threaded e o GC imprevisível do V8 tornam impossível manter tick rate estável a 60Hz com 100 entidades sob carga.

---

### ✅ ELOGIO — D3: WebRTC/UDP como Transporte de Combate

Priorizar fail-fast sobre consistência de pacotes no fluxo de combate é a decisão correta. Em combate de precisão, é melhor perder um frame de snapshot do que travar o rendering esperando retransmissão TCP. Isso alinha com o que todo FPS/action competitivo moderno faz (Valorant, Overwatch 2).

**Refinamento sugerido:** Considere **WebTransport** como substituto direto do WebRTC Data Channels no browser. WebTransport (HTTP/3 + QUIC) oferece as mesmas streams unreliable/unordered **sem necessidade de servidores STUN/TURN**. Handshake em ~100ms vs. ~1-3s do ICE negotiation do WebRTC.

---

### ✅ ELOGIO — D4: Godot como Terminal de Renderização

Excelente decisão. Manter o Godot estritamente como "tela burra" que consome snapshots e aplica interpolação + prediction é o gold standard de arquitetura de multiplayer. Isso garante que a lógica autoritativa nunca vaza para o cliente e que o backend é engine-agnostic — se você precisar trocar de engine no futuro, o servidor não muda.

---

## Categoria 4: Sustentabilidade e Testabilidade

---

### ✅ ELOGIO — T1: Funções Puras para Cálculo de Combate

Isso é a decisão mais valiosa da sua arquitetura inteira. Se `CalculateDamage(attacker, target, skill) → DamageResult` é uma função pura, sem side effects e sem dependências de I/O, você pode:

```go
// Unit test: prova que 1v5 é possível com skill
func TestHighSkillPlayerSurvives1v5(t *testing.T) {
    attacker := NewEntity(Stats{ATK: 500, DEF: 200, DodgeRate: 0.4})
    enemies := make([]Entity, 5)
    for i := range enemies {
        enemies[i] = NewEntity(Stats{ATK: 300, DEF: 150, DodgeRate: 0.1})
    }

    sim := NewCombatSimulation(attacker, enemies, seed: 42)
    result := sim.RunToCompletion()

    // O jogador habilidoso deve ganhar >30% das simulações
    assert.True(t, result.AttackerWinRate > 0.3)
}
```

Isso é impossível de fazer se a lógica de combate estiver entrelaçada com I/O de rede ou chamadas ao Redis.

---

### 🔴 BLOQUEADOR — T2: Sem Estratégia de Replay/Determinismo para Disputa de Resultados

**A falha:** Em um jogo onde 1v5 é possível, jogadores VÃO disputar resultados ("eu esquivei mas tomei hit"). Sem um sistema de replay server-side, você não tem como provar que o servidor estava correto.

**Correção — Event Sourcing Leve:**

Grave todos os inputs processados pelo servidor em uma fila (NATS JetStream) com timestamps:

```
{tick: 14502, entity: 42, input: "skill_Q", target: 87, server_time: 1750523829147}
{tick: 14502, entity: 87, input: "dodge_left", server_time: 1750523829149}
{tick: 14503, result: "hit", attacker: 42, target: 87, damage: 347, reason: "dodge_arrived_2ms_late"}
```

Com essas entradas + as funções puras de combate (T1), você pode **reprocessar qualquer luta offline** e gerar um replay frame-a-frame. Isso serve para:
- Sistema de replay in-game (espectador)
- Resolução de tickets de suporte ("por que eu morri?")
- Detecção de cheats offline (reprocessa luta e compara com resultado reportado)

**Trade-off:** Armazenamento. A ~60Hz com 100 players, isso gera ~50KB/s de dados. Para uma arena de 10 minutos = ~30MB. Armazene os últimos 7 dias e purgue automaticamente.

---

### ⚠️ MELHORIA — T3: YAGNI — Kubernetes/Agones no MVP

**A falha:** A stack proposta (K8s + Agones + NATS JetStream + múltiplos microsserviços) é a arquitetura correta para produção com milhares de jogadores. Mas para um MVP/Alpha com 50-200 testers, essa infra:
- Aumenta o tempo de setup em semanas
- Adiciona debugging complexity (pods, networking policies, service mesh)
- Custa ~$500-1000/mês em cloud só para rodar o cluster K8s mínimo

**Correção — Arquitetura evolutiva em 3 fases:**

| Fase | Jogadores | Infra | Justificativa |
|:---|:---|:---|:---|
| **MVP / Alpha** | 50-200 | Docker Compose em **1 VPS** (Hetzner ~$40/mês) | Foco em gameplay. Debug fácil. Tudo roda na mesma máquina |
| **Beta fechado** | 500-2000 | Docker Swarm ou **Fly.io** | Escala simples sem complexidade de K8s. Deploy em 1 comando |
| **Produção** | 2000+ | **Kubernetes + Agones** | Justificativa real para a complexidade. Auto-scaling de World Servers |

**Regra YAGNI:** Não escreva o Helm chart antes de ter 20 jogadores testando o combate.

---

### ⚠️ MELHORIA — T4: Teste de Integração de Rede — Simulação de Latência

**A falha:** Funções puras testam a lógica de combate, mas não testam o **comportamento do sistema sob latência real**. O bug mais comum em multiplayer não é lógico — é temporal (race conditions, pacotes fora de ordem, reconexão durante um trade).

**Correção — Teste com proxy de latência artificial:**

Adicione um proxy como **Toxiproxy** (da Shopify) no pipeline de CI que injeta:
- Latência fixa (50ms, 100ms, 200ms)
- Jitter aleatório (±30ms)
- Perda de pacotes (5%, 10%)
- Desconexão abrupta no meio de uma skill

E valide que:
- O servidor nunca crasha
- O jogador reconecta e retoma o estado correto
- Nenhum item é duplicado ou perdido durante desconexão no meio de trade

---

## Resumo Executivo

| # | Finding | Classificação | Impacto |
|:--|:--------|:---|:---|
| P1 | Redis no game loop — latência mata o tick | 🔴 Bloqueador | Estado in-process, Redis só cache |
| P2 | LoS/Colisão O(N²) em arena de 100 | 🔴 Bloqueador | Spatial Hash + LoS 2D simplificado |
| P3 | Snapshot bandwidth sem delta/AOI | ⚠️ Melhoria | Delta compression + Interest Management |
| S1 | Peeker's advantage sem lag compensation | 🔴 Bloqueador | Server-side rewind com buffer 200ms |
| S2 | Dodge spoofing com timestamps falsos | 🔴 Bloqueador | Rejeição de timestamps + cooldown server |
| S3 | Input flood via WebRTC | ⚠️ Melhoria | Rate limiter no Gateway (30 inputs/s) |
| D1 | Controller/Service/Repo inadequado p/ game loop | 🔴 Bloqueador | ECS para World Server, CSR para microsserviços |
| D2 | Linguagem do servidor indefinida | ⚠️ Melhoria | Go (performance) ou C# (ecossistema). Node.js eliminado |
| D3 | WebRTC/UDP para combate | ✅ Elogio | Refinar para WebTransport |
| D4 | Godot como terminal de render | ✅ Elogio | — |
| T1 | Funções puras para combate | ✅ Elogio | Fundação correta para testabilidade |
| T2 | Sem replay/event sourcing | 🔴 Bloqueador | Gravar inputs + reprocessar offline |
| T3 | K8s/Agones no MVP | ⚠️ Melhoria | Docker Compose → Swarm/Fly → K8s evolutivo |
| T4 | Sem teste de integração com latência | ⚠️ Melhoria | Toxiproxy no CI |
