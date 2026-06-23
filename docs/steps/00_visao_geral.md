# 00 — Visão Geral do Projeto Weapons Masters

## O que é

MMORPG cross-platform (PC, Mobile, Web) com combate **Tab-Target híbrido** (estilo Guild Wars 2): skills são direcionadas ao alvo selecionado, mas o alvo pode esquivar ativamente com dodge roll + i-frames. A habilidade do jogador permite vencer confrontos 1v5.

## Stack Definitiva

| Camada | Tecnologia |
|:---|:---|
| Cliente | **Godot 4.x + C#** (exporta para Web/WebGPU, PC, Android, iOS) |
| Servidor (World Server + Gateway) | **Rust** (tokio, bevy_ecs, rapier2d, wtransport) |
| Serialização | **Protobuf** (.proto compartilhado → prost no Rust, Google.Protobuf no C#) |
| Protocolo Externo | **WebTransport** (browser) + **KCP/UDP** (PC/Mobile) |
| Protocolo Interno | **TCP raw + Protobuf** (gameplay) / **gRPC** (serviços) |
| Fila Durável | **NATS JetStream** |
| Cache | **Redis** (sessões, AOI, ranking — **nunca** como fonte de estado) |
| Banco de Dados | **PostgreSQL + JSONB** (fonte da verdade ACID) |
| Observabilidade | **Grafana + Prometheus + Loki** |
| Dev Local | **Docker Compose** (server + postgres + redis + nats) |

## 3 Regras de Ouro

1. **RAM do World Server = fonte da verdade durante gameplay.** PostgreSQL = fonte da verdade permanente. Redis = apenas cache.
2. **O cliente é uma tela burra.** Ele envia *intents* ("quero mover Norte"). O servidor valida, processa e devolve o resultado. O cliente só renderiza.
3. **Cada Step termina com algo jogável.** Se não dá pra abrir e testar, o Step não está pronto.

## Arquitetura (Diagrama Final)

```
                    ┌──────────────────────────────┐
                    │     Load Balancer L4          │
                    │     (HAProxy / NLB)           │
                    └──────────┬───────────────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
        ┌─────┴─────┐  ┌──────┴─────┐  ┌──────┴──────┐
        │ Gateway #1 │  │ Gateway #2 │  │ Gateway #N  │
        │   (Rust)   │  │   (Rust)   │  │   (Rust)    │
        └─────┬──────┘  └──────┬─────┘  └──────┬──────┘
              │   TCP raw + Protobuf    │               │
      ┌───────┴────────────────┴────────┴───────┐
      │                                         │
┌─────┴──────┐  ┌────────────┐  ┌──────────────┐
│ World Srv  │  │ Auth Srv   │  │ Economy Srv  │
│ (ECS/Rust) │  │ (Rust/gRPC)│  │ (Rust/gRPC)  │
└─────┬──────┘  └─────┬──────┘  └──────┬───────┘
      │               │               │
      ├───── NATS JetStream ───────────┤
      │               │               │
      │         ┌─────┴──────┐        │
      │         │ DB Sync    │        │
      │         │ Worker     │        │
      │         └─────┬──────┘        │
      │               │               │
      ▼               ▼               ▼
   Redis         PostgreSQL       PostgreSQL
  (cache)      (fonte verdade)   (economia)
```

## Fluxo de um Input (Resumo)

```
1. Jogador aperta "W"
2. Cliente aplica prediction (move local imediatamente)
3. Cliente serializa PlayerInput → Protobuf → envia via WebTransport
4. Gateway recebe → repassa via TCP raw ao World Server
5. World Server valida (speed check, rate limit) → atualiza posição na RAM
6. World Server monta snapshot delta → envia ao Gateway
7. Gateway repassa snapshot ao cliente
8. Cliente reconcilia posição local vs. snapshot do servidor
```

## Modelo de Combate (Tab-Target + Dodge Ativo)

```
1. Player A aperta Tab → seleciona Player B como alvo
2. Player A aperta "Q" → Intent "usar skill Disparo no alvo B"
3. Servidor verifica:
   - B está no range da skill? (distância euclidiana 2D)
   - Há LoS entre A e B? (raycast 2D contra obstáculos)
   - B está em i-frames de dodge? (invulnerabilidade ativa?)
   - Lag compensation: rebobina posição de B para o momento que A viu a cena
4. Se tudo OK → aplica dano → envia DamageEvent ao cliente
5. Se B esquivou a tempo → envia DodgeResult ao cliente
```

## Próximo Passo

Leia os arquivos na ordem: `01_step1.md` → `02_step2.md` → `03_step3.md` → `04_step4.md` → `05_step5.md`. Cada arquivo contém tudo que você precisa para completar aquele Step, incluindo checklist de tarefas, código exemplo e critério de "pronto".
