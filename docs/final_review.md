# Revisão Final Consolidada — Weapons Masters

> [!IMPORTANT]
> Esta revisão cruza os 5 documentos existentes, encontra **inconsistências entre eles** e cobre **lacunas que nenhum documento abordou** até agora.

---

## Parte 1: Inconsistências entre Documentos (Devem ser Corrigidas)

---

### 🔴 INC-1: Stack conflitante — Documentos dizem Go, decisão é Rust

**Onde:** `mmorpg_2026_architecture_guide.md` (linhas 28, 240) ainda referencia "Go / Rust" no Gateway e "Go (goroutines)" como tecnologia primária. `architecture_stress_test.md` (linhas 52-68, 107-127) usa exemplos de código em **Go**, não Rust.

**Correção:** A decisão final é **Rust para todo o backend** (World Server + Gateway + microsserviços). Os exemplos de código em Go (SpatialGrid, CombatService, lag compensation) devem ser reescritos em Rust. A tabela da seção 9 do guide deve refletir:

| Componente | Tecnologia |
|:---|:---|
| Gateway | **Rust** (tokio + wtransport + kcp) |
| World Server | **Rust** (bevy_ecs + rapier2d) |
| Microsserviços | **Rust** (tonic para gRPC) |
| Cliente | **Godot 4.x + C#** |

---

### ⚠️ INC-2: Tab-Target vs. Esquiva Ativa — Definição conflitante

**Onde:** O pedido do usuário diz "Tab-Target de alta precisão técnica" com "strafing e esquiva ativa". Isso é uma **contradição parcial** que nenhum documento resolveu explicitamente.

- **Tab-Target clássico** (WoW, FFXIV): Você seleciona o alvo e suas skills automaticamente acertam. A habilidade está em rotação de skills, timing de cooldowns e posicionamento.
- **Action combat** (BDO, New World): Você mira manualmente. A habilidade está em aim + dodge.

**Correção — Definir o modelo híbrido explicitamente:**

O Weapons Masters usa **Tab-Target com dodge ativo**, que é o modelo do **Guild Wars 2 / Throne and Liberty**:
- Skills são direcionadas ao alvo selecionado (Tab-Target) → sem aim manual
- Mas o alvo pode **esquivar ativamente** (dodge roll com i-frames) para evitar a skill
- **Line of Sight** importa: se há um obstáculo entre atacante e alvo, a skill falha
- **Range check** é autoritativo: a skill só conecta se o alvo estiver dentro do alcance no momento do cast no servidor

Isso simplifica enormemente o servidor: não precisa de raycasts de projéteis. O fluxo de hit check se torna:

```rust
fn check_tab_target_hit(
    attacker: &CombatState,
    target: &CombatState,
    skill: &SkillDef,
    spatial: &SpatialHash,
) -> HitResult {
    // 1. Range check (distância euclidiana 2D, barato)
    let dist = attacker.position.distance(target.position);
    if dist > skill.range {
        return HitResult::OutOfRange;
    }

    // 2. LoS check (ray 2D contra obstáculos AABB, barato)
    if spatial.is_blocked(attacker.position, target.position) {
        return HitResult::Blocked;
    }

    // 3. Dodge check (i-frames ativos?)
    if target.is_invulnerable() {
        return HitResult::Dodged;
    }

    // 4. Cálculo de dano (função pura)
    let damage = calculate_damage(attacker.stats, target.stats, skill);
    HitResult::Hit { damage }
}
```

**Impacto:** Isso elimina a necessidade de raycasts de projéteis complexos (P2 do stress test fica muito mais leve). O LoS check é um único raycast 2D por skill, não N² por tick.

---

### ⚠️ INC-3: `std::thread::sleep` para timing do game loop — Impreciso

**Onde:** `rust_godot_stack_analysis.md` (linha 259) usa `std::thread::sleep(TICK_DURATION - elapsed)`.

**Problema:** `thread::sleep` no Linux/Windows tem granularidade de ~1-15ms dependendo do OS scheduler. Se o tick budget é 33ms (30Hz) e o processamento levou 20ms, `sleep(13ms)` pode dormir 14ms ou 28ms, causando jitter no tick rate.

**Correção — Spin-wait híbrido:**

```rust
fn sleep_precise(target: Duration) {
    // Dorme grosseiramente até faltarem 2ms (evita queimar CPU)
    if target > Duration::from_millis(2) {
        std::thread::sleep(target - Duration::from_millis(2));
    }
    // Spin-wait para os últimos ~2ms (precisão de microsegundos)
    let deadline = Instant::now() + target;
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}
```

---

## Parte 2: Lacunas Não Cobertas em Nenhum Documento

---

### 🔴 GAP-1: Reconexão do Jogador — Zero documentação

**Cenário:** O jogador está no meio de um combate 1v5, o WiFi dele cai por 3 segundos e volta. O que acontece?

Nenhum documento aborda isso. Sem estratégia de reconexão, o jogador perde o personagem no servidor (timeout), volta ao login e precisa relogar. Em um combate de precisão, isso é inaceitável.

**Correção — Reconexão transparente em 3 fases:**

1. **Servidor:** Quando o Gateway detecta desconexão, o World Server **não remove** a entidade imediatamente. Ela entra em estado `Disconnected` com um timer de 30 segundos. O personagem fica parado (não pode ser atacado em PvP, mas mobs continuam atacando em PvE).

2. **Gateway:** Quando o cliente reconecta (novo handshake WebTransport/KCP), o Gateway valida o token JWT (que ainda é válido) e reassocia a sessão ao World Server original.

3. **Cliente:** Ao reconectar, o cliente recebe um snapshot completo (não delta) do estado atual e retoma a interpolação normalmente. O jogador vê um breve "Reconectando..." e volta ao combate.

```rust
// No World Server
fn handle_disconnect(world: &mut World, entity: Entity) {
    // NÃO remove. Marca como desconectado.
    world.insert(entity, Disconnected {
        since: Instant::now(),
        timeout: Duration::from_secs(30),
    });
    // Torna imune a PvP, mas não a PvE
    world.insert(entity, PvPImmune);
}

fn disconnection_cleanup_system(world: &mut World) {
    // Só remove após 30s sem reconexão
    for (entity, dc) in world.query::<&Disconnected>() {
        if dc.since.elapsed() > dc.timeout {
            world.despawn(entity);
            // Persiste estado final no PG via NATS
        }
    }
}
```

---

### 🔴 GAP-2: Graceful Shutdown do World Server — Perda de dados em deploy

**Cenário:** Você faz deploy de uma nova versão do World Server. O Kubernetes mata o pod. Os 200 jogadores no mapa perdem o progresso dos últimos 30 segundos (desde o último snapshot periódico).

**Correção — Shutdown hook com flush forçado:**

```rust
use tokio::signal;

// No main.rs, antes do game loop
let shutdown_tx = /* channel para sinalizar o game loop */;

tokio::spawn(async move {
    signal::ctrl_c().await.ok();
    // Kubernetes envia SIGTERM → capturado aqui
    tracing::info!("Shutdown signal received. Flushing all player state...");
    shutdown_tx.send(()).ok();
});

// No game loop, ao receber shutdown signal:
fn shutdown_flush(world: &World, nats: &NatsClient) {
    // Snapshot COMPLETO de todos os jogadores (não delta)
    for (entity, player) in world.query::<&PlayerState>() {
        let snapshot = serialize_full_snapshot(player);
        nats.publish("persistence.flush", snapshot); // Fila durável
    }
    // Espera ACK de todos antes de sair (max 10s)
    nats.flush_with_timeout(Duration::from_secs(10));
}
```

**Kubernetes config:**
```yaml
terminationGracePeriodSeconds: 30  # Dá 30s para flush antes de SIGKILL
```

---

### 🔴 GAP-3: Entrega de Assets para Web — Sem CDN/estratégia de download

**Cenário:** O jogo Godot exportado para Web com WebGPU gera um bundle de ~200-500MB (texturas, modelos 3D, áudio). O jogador abre o navegador e precisa baixar tudo antes de jogar.

Nenhum documento aborda isso.

**Correção — Asset Streaming via CDN:**

1. **Build:** Exporte o Godot com `.pck` split por região/mapa. Main bundle contém apenas UI + tela de login + mapa inicial (~50MB).
2. **CDN:** Sirva os `.pck` adicionais via **Cloudflare R2** ou **AWS CloudFront**. O cliente baixa mapas adicionais em background enquanto o jogador está no lobby/cidade.
3. **Loading progressivo:** Mostre uma barra de progresso estilizada enquanto o mapa seguinte carrega. O jogador pode interagir com NPC/chat/inventário enquanto assets carregam.

```csharp
// Godot C# — download de pack adicional em background
public async Task LoadMapPackAsync(string mapName)
{
    string url = $"https://cdn.weapons-masters.com/packs/{mapName}.pck";
    var http = new HttpClient();
    byte[] data = await http.GetByteArrayAsync(url);
    
    string localPath = $"user://packs/{mapName}.pck";
    System.IO.File.WriteAllBytes(
        ProjectSettings.GlobalizePath(localPath), data
    );
    
    bool ok = ProjectSettings.LoadResourcePack(localPath);
    GD.Print(ok ? $"Pack {mapName} loaded" : $"Pack {mapName} failed");
}
```

---

### ⚠️ GAP-4: Console Support (PlayStation/Xbox/Switch) — Implicações não mapeadas

O requisito original menciona "Console". Godot 4.x **não tem suporte oficial a consoles** — precisa de portes por terceiros (ex: **Lone Wolf Technology** para Switch, **Pineapple Works** para PS/Xbox). Isso significa:
- Custo adicional de licenciamento (~$3000-10000 por plataforma)
- Processo de certificação Sony/Microsoft/Nintendo (3-6 meses)
- Controles de input diferentes (gamepad layout, haptics)

**Recomendação:** Deixe console para uma segunda fase. Lance primeiro em **Web + PC + Mobile**, valide o gameplay e a economia, e então invista no porte para consoles quando houver receita.

---

### ⚠️ GAP-5: Economia do Jogo — Gold Sinks não definidos

O stress test menciona monitorar "gold sink ratio", mas nenhum documento define **quais são os gold sinks** do Weapons Masters. Sem sinks, a inflação destrói a economia em semanas.

**Gold sinks mínimos para um MMORPG:**

| Sink | Descrição | % do gold removido (estimado) |
|:---|:---|:---|
| Reparo de equipamento | Após morte ou uso prolongado | 20-30% |
| Taxa de Auction House | 5% sobre cada venda | 15-25% |
| Enhance/Upgrade de item | Custo crescente por nível | 20-30% |
| Teleporte rápido | Custo por distância | 5-10% |
| Consumíveis (poções) | Compra de NPCs | 10-15% |

**Regra:** Gold gerado por hora (farm de mobs + quests) deve ser ≈ gold removido por hora no jogador médio. Monitore isso com a métrica do Grafana.

---

### ⚠️ GAP-6: Escala de Banco — PostgreSQL sem particionamento

O guide define PostgreSQL como fonte de verdade, mas não menciona **o que acontece quando a tabela `player_inventory` tem 50 milhões de linhas** (10.000 jogadores × 200 itens × 25 characters).

**Correção:** Planeje particionamento desde o schema inicial:

```sql
-- Particionamento por range de player_id
CREATE TABLE player_inventory (
    player_id   BIGINT NOT NULL,
    slot        SMALLINT NOT NULL,
    item_data   JSONB NOT NULL,
    updated_at  TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (player_id, slot)
) PARTITION BY HASH (player_id);

-- 16 partições (ajustável conforme crescimento)
CREATE TABLE player_inventory_p0 PARTITION OF player_inventory
    FOR VALUES WITH (MODULUS 16, REMAINDER 0);
-- ... p1 a p15
```

---

### ⚠️ GAP-7: Autenticação — JWT sem refresh token é vulnerável

O guide menciona JWT para sessões. Se o JWT é roubado (XSS no browser, man-in-the-middle), o atacante tem acesso permanente até o token expirar.

**Correção:**
- JWT com expiração **curta** (15 minutos)
- **Refresh token** opaco armazenado no Redis com TTL de 7 dias
- Refresh token é rotacionado a cada uso (one-time use)
- Se o servidor detectar uso de um refresh token já invalidado → revoga toda a sessão (possível roubo)

---

## Parte 3: Resumo Executivo da Revisão Final

| # | Item | Tipo | Ação |
|:--|:-----|:-----|:-----|
| INC-1 | Stack Go nos docs, decisão é Rust | 🔴 Inconsistência | Atualizar todos os exemplos e tabelas para Rust |
| INC-2 | Tab-Target vs. Action não definido | ⚠️ Ambiguidade | Modelo híbrido (GW2-style) documentado acima |
| INC-3 | `thread::sleep` impreciso | ⚠️ Técnico | Spin-wait híbrido |
| GAP-1 | Reconexão do jogador | 🔴 Lacuna crítica | Estado `Disconnected` + 30s timeout + JWT reuse |
| GAP-2 | Graceful shutdown | 🔴 Lacuna crítica | SIGTERM hook + flush forçado via NATS |
| GAP-3 | Assets Web (CDN) | 🔴 Lacuna crítica | Split `.pck` + CDN + loading progressivo |
| GAP-4 | Console support | ⚠️ Info | Segunda fase. Custos e prazos mapeados |
| GAP-5 | Gold sinks | ⚠️ Game design | 5 sinks mínimos definidos |
| GAP-6 | PG sem particionamento | ⚠️ Escala | Hash partition por player_id |
| GAP-7 | JWT sem refresh token | ⚠️ Segurança | Short-lived JWT + rotating refresh token |
