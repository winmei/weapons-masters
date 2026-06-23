# Step 2 - Combate Tab-Target entre 2 Jogadores

## Objetivo

Dois clientes Godot entram no mesmo mundo, selecionam o outro jogador com Tab e usam habilidades hardcoded. O servidor Rust e a unica autoridade para range, line of sight, cooldown, i-frames, dano e morte.

Nao entram neste passo: mobs, progressao, inventario ou PostgreSQL.

## Estado da implementacao

- Contratos de rede atualizados em `proto/game_messages.proto`.
- `PlayerInput` agora carrega `SkillUse`, `DodgeInput` e `client_tick`.
- `WorldSnapshot` agora carrega `CombatEvent` e `local_entity_id` preenchido pelo Gateway por conexao.
- `EntityState` agora inclui HP, HP maximo e `EntityAction`.
- Gateway WebTransport e WebSocket impoem a identidade autoritativa da conexao e ignoram `entity_id` declarado pelo cliente.
- Servidor `world` migrado para `bevy_ecs::Schedule`, com sistemas encadeados para input, cleanup, movimento, spatial hash, combate, historico e snapshot.
- Input de rede agora atualiza apenas `MovementIntent`; a translacao ocorre uma unica vez por tick global, bloqueando speedhack por acumulacao de pacotes.
- Jogador quebrado em componentes: `NetworkIdentity`, `Position`, `Health`, `CombatState`, `DodgeState`, `MovementIntent`, `LastActive` e `PositionHistory`.
- Entidades sem input por mais de 5 segundos sao removidas do ECS e do indice de entidades.
- `SnapshotCache` reaproveita o vetor de entidades entre ticks antes de montar o `WorldSnapshot`.
- Spatial hash implementada como flat array 64x64 com linked list intrinseca.
- LoS usa validacao servidor-side contra a parede da arena.
- `PositionHistory` usa ring buffer de 12 entradas, cobrindo aproximadamente 400ms em 30Hz.
- `check_hit` e uma funcao pura e testavel.
- Habilidades hardcoded:
  - `Golpe`: skill `1`, range 3m, 50 dano, cooldown 1.5s.
  - `Disparo`: skill `2`, range 15m, 80 dano, cooldown 3s, cast de 500ms.
- Dodge servidor-side:
  - translacao de 3m;
  - i-frames de 300ms;
  - cooldown de 1.5s;
  - evento `DodgeResult` emitido pelo servidor.
- Cliente Godot C# envia:
  - Tab para selecionar alvo;
  - `1` para Golpe;
  - `2` para Disparo;
  - Espaco para dodge.
- Cliente renderiza:
  - indicador sob o alvo selecionado;
  - `Label3D` de HP sobre jogadores;
  - HUD estatico de HP local com `ProgressBar`;
  - target frame estatico com `ProgressBar`;
  - texto flutuante para dano, MISS e morte.

## Arquivos principais

- `proto/game_messages.proto`
- `server/Cargo.toml`
- `server/crates/world/Cargo.toml`
- `server/crates/world/src/main.rs`
- `client/project.godot`
- `client/scenes/Main.tscn`
- `client/scripts/Network/InputSender.cs`
- `client/scripts/Network/NetworkManager.cs`
- `client/scripts/Network/PacketHandler.cs`
- `client/scripts/Prediction/ClientPrediction.cs`

## Contratos de rede adicionados

- `SkillUse`
- `DodgeInput`
- `DamageEvent`
- `DodgeResult`
- `DeathEvent`
- `CombatEvent`
- `EntityAction`

## Criterio de pronto

O Step 2 e considerado funcional quando dois clientes conectados conseguem:

1. Ver o outro jogador no mundo.
2. Selecionar alvo com Tab.
3. Usar `1` ou `2` contra o alvo.
4. Fazer dodge com Espaco.
5. Receber do servidor o resultado autoritativo: dano, MISS ou morte.
6. Ver HP e feedback visual atualizados pelo snapshot/eventos do servidor.

## Observacao sobre validacao local

Este repositorio depende das toolchains `cargo`, `dotnet` e `godot` instaladas no PATH para validacao completa. Sem elas, a validacao possivel neste ambiente fica restrita a revisao estatica dos arquivos.
