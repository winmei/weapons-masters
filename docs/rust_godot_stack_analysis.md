# Stack Definida: Rust (Server) + Godot C# (Client)

---

## 1. Por que essa combinação é forte

| Propriedade | Rust (Server) | Godot 4.x + C# (Client) |
|:---|:---|:---|
| **GC pauses** | Zero. Sem garbage collector. Tick loop previsível em microsegundos | C# tem GC, mas no cliente isso é aceitável — frames dropped de GC são mascarados pela interpolação |
| **Concorrência** | `tokio` async + `rayon` para paralelismo de dados. Zero data races garantido pelo compilador | Godot gerencia threads internamente. C# `async/await` para I/O de rede |
| **Memória** | Ownership model impede leaks e use-after-free. Binário de ~5-15MB | Godot runtime ~50MB. Aceitável para todas as plataformas |
| **Cross-compile** | `cargo build --target` gera binário Linux x86 para servidor. Sem runtime | Godot exporta para Web (WASM), PC, Android, iOS, Console |

**O risco principal:** Rust tem curva de aprendizado íngreme. O borrow checker vai rejeitar padrões comuns de game dev (referências mutáveis cruzadas entre entidades). A solução é o padrão ECS, que o ecossistema Rust domina.

---

## 2. Ecossistema Rust para Game Server (2026)

| Necessidade | Crate Recomendada | Alternativa |
|:---|:---|:---|
| **ECS (Entity-Component-System)** | **`bevy_ecs`** (extraído do Bevy, usável standalone) | `hecs` (mais leve, sem overhead do Bevy completo) |
| **Async Runtime** | **`tokio`** | — |
| **WebTransport (Web clients)** | **`wtransport`** | `h3-webtransport` |
| **UDP/KCP (Native clients)** | **`kcp`** + **`tokio-udp`** | `laminar`, `naia` |
| **Serialização (Protobuf)** | **`prost`** (geração de código) | `flatbuffers` (zero-copy, mais rápido, mais complexo) |
| **Spatial Hash** | Implementação própria (~100 linhas) | `spatial_hash` crate |
| **Física 2D server-side** | **`rapier2d`** (pela Dimforge, madura) | `parry2d` (só colisão, sem simulação) |
| **Logging estruturado** | **`tracing`** + `tracing-loki` | — |
| **Métricas Prometheus** | **`metrics`** + `metrics-exporter-prometheus` | — |
| **PostgreSQL** | **`sqlx`** (async, compile-time checked queries) | `tokio-postgres` |
| **Redis** | **`fred`** (async, cluster-ready) | `redis-rs` |
| **Message Broker (NATS)** | **`async-nats`** (JetStream built-in) | — |

---

## 3. O Problema da Serialização Compartilhada (Rust ↔ C#)

Rust e C# não compartilham tipos nativamente. Todo pacote de rede precisa de um formato intermediário. A solução padrão é **Protocol Buffers (Protobuf)** com geração de código para ambas as linguagens a partir de um `.proto` único:

```
projeto/
├── proto/                          ← FONTE DA VERDADE (compartilhada)
│   ├── game_messages.proto
│   ├── auth.proto
│   └── economy.proto
├── server/                         ← Rust
│   └── (prost gera structs a partir dos .proto)
└── client/                         ← Godot C#
    └── (protobuf-net ou Google.Protobuf gera classes)
```

### Exemplo: Definição de pacote compartilhada

```protobuf
// proto/game_messages.proto
syntax = "proto3";
package mmorpg;

message PlayerInput {
  uint32 sequence = 1;         // Número de sequência para reconciliation
  InputType type = 2;
  Vec2 direction = 3;          // Direção do movimento/dodge
  uint32 target_entity_id = 4; // Alvo da skill (tab-target)
  uint32 skill_id = 5;
}

message WorldSnapshot {
  uint32 tick = 1;
  uint32 last_processed_input = 2;  // Para client reconciliation
  repeated EntityState entities = 3;
}

message EntityState {
  uint32 entity_id = 1;
  Vec2 position = 2;
  float rotation = 3;
  int32 hp = 4;
  int32 max_hp = 5;
  EntityAction current_action = 6;  // IDLE, MOVING, CASTING, DODGING
  repeated ActiveBuff buffs = 7;
}

message Vec2 {
  float x = 1;
  float y = 2;
}

enum InputType {
  MOVE = 0;
  SKILL = 1;
  DODGE = 2;
  STOP = 3;
  INTERACT = 4;
}
```

### Build pipeline:

```toml
# server/Cargo.toml
[build-dependencies]
prost-build = "0.13"
```

```rust
// server/build.rs — gera structs Rust automaticamente no compile
fn main() {
    prost_build::compile_protos(
        &["../proto/game_messages.proto", "../proto/auth.proto"],
        &["../proto/"],
    ).unwrap();
}
```

```xml
<!-- client/Weapons Masters Client.csproj — gera classes C# -->
<ItemGroup>
  <Protobuf Include="../proto/*.proto" GrpcServices="None" />
</ItemGroup>
<PackageReference Include="Google.Protobuf" Version="3.*" />
<PackageReference Include="Grpc.Tools" Version="2.*" PrivateAssets="All" />
```

**Resultado:** Um único arquivo `.proto` gera tipos idênticos em Rust e C#. Mudar o formato de um pacote = editar o `.proto` → `cargo build` + rebuild no Godot → ambos os lados atualizados automaticamente.

---

## 4. Estrutura de Projeto Recomendada

```
mmorpg-project/
│
├── proto/                              # Contratos de rede (Protobuf)
│   ├── game_messages.proto
│   ├── auth.proto
│   └── economy.proto
│
├── server/                             # Rust workspace
│   ├── Cargo.toml                      # [workspace] com members
│   ├── crates/
│   │   ├── world/                      # Game Loop + World State
│   │   │   ├── src/
│   │   │   │   ├── ecs/                # World, Entity, Components
│   │   │   │   ├── systems/            # MovementSystem, CombatSystem, BuffSystem
│   │   │   │   ├── combat/             # damage_calc, skill_validation (puras)
│   │   │   │   └── spatial/            # SpatialHash, AOI
│   │   │   └── Cargo.toml
│   │   │
│   │   ├── gateway/                    # Gateway: WebTransport + Protobuf
│   │   │   ├── src/
│   │   │   │   ├── gateway.rs          # Aceita conexões, traduz protocolo
│   │   │   │   ├── session.rs          # Estado de sessão por jogador
│   │   │   │   └── rate_limiter.rs     # Anti-flood (30 inputs/s)
│   │   │   └── Cargo.toml
│   │   │
│   │   ├── persistence/                # DB Sync: NATS consumer → PostgreSQL
│   │   │   ├── src/
│   │   │   │   ├── snapshot_writer.rs   # Batch UPSERT periódico
│   │   │   │   └── event_writer.rs      # Eventos críticos (trades, drops)
│   │   │   └── Cargo.toml
│   │   │
│   │   ├── services/                   # Microsserviços (Auth, Social, Economy)
│   │   │   └── Cargo.toml
│   │   │
│   │   └── anticheat/                  # Lag compensation + análise async
│   │       └── Cargo.toml
│   │
│   ├── build.rs                        # Compila .proto → Rust structs
│   └── src/main.rs                     # Entrypoint: inicia tokio + game loop
│
├── client/                             # Godot 4.x + C#
│   ├── project.godot
│   ├── Weapons Masters Client.csproj   # Refs ao Protobuf + Google.Protobuf
│   ├── scripts/
│   │   ├── Network/
│   │   │   ├── NetworkManager.cs       # WebTransport/KCP connection
│   │   │   ├── PacketHandler.cs        # Deserializa snapshots Protobuf
│   │   │   └── InputSender.cs          # Serializa inputs → envia
│   │   ├── Prediction/
│   │   │   ├── ClientPrediction.cs     # Aplica input localmente
│   │   │   └── Reconciliation.cs       # Corrige desvio vs. snapshot do server
│   │   ├── Rendering/
│   │   │   ├── EntityInterpolator.cs   # Interpola posições entre snapshots
│   │   │   └── EffectsManager.cs       # VFX de skills, hit feedback
│   │   └── UI/
│   │       ├── HUD.cs
│   │       └── TargetFrame.cs          # Tab-target UI
│   ├── scenes/
│   ├── assets/
│   └── export_presets.cfg              # Web (WASM), PC, Android, iOS
│
├── docker/
│   ├── Dockerfile.server               # Multi-stage: rust builder → alpine
│   ├── Dockerfile.dbsync               # Persistence worker
│   └── compose.yml                     # Dev local: server + postgres + redis + nats
│
├── infra/                              # IaC (fase de produção)
│   ├── k8s/
│   └── terraform/
│
└── tests/
    ├── combat_simulations/             # Testes das funções puras (cargo test)
    ├── integration/                    # Testes com Toxiproxy (latência)
    └── replays/                        # Event sourcing replay validation
```

---

## 5. O Game Loop em Rust (Esqueleto)

```rust
// server/src/main.rs
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const TICK_RATE: u64 = 30; // Hz
const TICK_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TICK_RATE);

#[tokio::main]
async fn main() {
    // Canais para receber inputs do Gateway (thread de rede separada)
    let (input_tx, mut input_rx) = mpsc::channel::<PlayerInput>(4096);

    // Inicia gateway em task async separada
    let gateway = tokio::spawn(gateway::run(input_tx));

    // Game loop roda em thread dedicada (NÃO no tokio runtime)
    let game_loop = std::thread::spawn(move || {
        let mut world = ecs::World::new();
        let mut spatial = spatial::SpatialHash::new(20.0);
        let mut tick: u64 = 0;

        loop {
            let tick_start = Instant::now();

            // 1. Drena todos os inputs que chegaram desde o último tick
            while let Ok(input) = input_rx.try_recv() {
                systems::input::process(&mut world, input);
            }

            // 2. Executa sistemas na ordem correta
            systems::movement::run(&mut world, &mut spatial);
            systems::combat::run(&mut world, &spatial);  // usa spatial para LoS
            systems::buffs::run(&mut world);
            systems::cleanup::run(&mut world);            // remove entidades mortas

            // 3. Monta snapshot delta e envia para o Gateway broadcast
            let snapshot = snapshot::build_delta(&world, tick);
            // broadcast_tx.send(snapshot) — envia para task de rede

            // 4. Event sourcing: grava inputs processados neste tick
            // nats_tx.send(tick_events)

            tick += 1;

            // 5. Dorme pelo tempo restante do tick
            let elapsed = tick_start.elapsed();
            if elapsed < TICK_DURATION {
                std::thread::sleep(TICK_DURATION - elapsed);
            } else {
                tracing::warn!(
                    "Tick {} overran by {:?}", tick, elapsed - TICK_DURATION
                );
            }
        }
    });

    let _ = tokio::join!(gateway);
    game_loop.join().unwrap();
}
```

**Detalhe crucial:** O game loop roda em `std::thread::spawn`, **não** em uma task do tokio. O tokio runtime é para I/O assíncrono (rede, banco). O game loop precisa de timing determinístico que o scheduler do tokio não garante.

---

## 6. Riscos Específicos desta Combinação

| Risco | Severidade | Mitigação |
|:---|:---|:---|
| Curva do borrow checker no ECS | ⚠️ Média | Use `bevy_ecs` que já resolve ownership patterns. Evite `Rc<RefCell<T>>` — se precisou disso, o design está errado |
| Godot C# + Web export (WASM) | ⚠️ Média | Godot 4.x exporta C# para Web via .NET WASM, mas performance é ~60-70% do nativo. Teste cedo com arenas lotadas no browser |
| Debug cross-language (Rust ↔ C#) | ⚠️ Média | Logs estruturados com `tracing` (Rust) + packet inspector no client. Nunca dependa de printf-debugging em rede |
| `unsafe` em Rust | 🔴 Alta se abusado | Regra absoluta: zero `unsafe` no código do jogo. Se precisar, encapsule em crate separada com testes de fuzzing |
| Protobuf overhead para pacotes pequenos | ⚠️ Baixa | Protobuf adiciona ~2-5 bytes de overhead por campo. Para snapshots com 100 entidades (~6KB), isso é irrelevante. Se virar problema, migre para FlatBuffers |
