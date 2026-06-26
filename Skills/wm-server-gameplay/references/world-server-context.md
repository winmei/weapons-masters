# World Server - contexto

Arquivos principais:

- `server/crates/world/src/main.rs`
- `server/crates/world/src/mobs.rs`
- `server/crates/world/src/experience.rs`
- `proto/game_messages.proto`

## Schedule atual

O World Server cria um `bevy_ecs::World`, insere recursos e roda um `Schedule` encadeado:

1. `process_network_inputs_system`
2. `cleanup_disconnected_system`
3. `apply_movement_and_dodge_system`
4. `rebuild_spatial_hash_system`
5. `process_player_combat_skills_system`
6. `mobs::update_player_positions_system`
7. `mobs::mob_ai_system`
8. `apply_mob_attacks_system`
9. `mobs::apply_mob_respawn_system`
10. `record_position_history_system`
11. `experience::experience_system`
12. `build_and_broadcast_snapshot_system`

## Constantes importantes

- Tick: 30Hz.
- Velocidade do player: 5 unidades/s.
- Dodge: distancia 3, i-frame 300ms, cooldown 1500ms.
- Historico de posicao: 12 amostras.
- Skills hardcoded: `GOLPE` id 1, `DISPARO` id 2.

## Modelo de combate

- `SkillUse` chega via `PlayerInput`.
- `pending_skill` fica no `CombatState`.
- Cooldown e cast sao atualizados server-side.
- `check_hit` valida range, LoS e dodge.
- Eventos de dano, dodge e morte saem em `WorldSnapshot.combat_events`.

## Mobs e progressao

- Mobs usam `MobState`: `Idle`, `Aggro`, `Attack`, `Dead`.
- Existem definicoes estaticas de mobs e loot tables.
- XP passa por `RewardEventQueue` e `experience_system`.
- Quando adicionar inventario real, separar evento de gameplay da escrita em banco.
