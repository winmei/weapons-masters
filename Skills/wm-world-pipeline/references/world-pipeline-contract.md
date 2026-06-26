# World pipeline contract

## Fonte de autoria

O Godot deve conter a representacao visual e pontos de autoria do mundo:

- Nodes marcadores para spawn e transicao.
- Collision shapes ou meshes marcadas para export server-side.
- Areas/volumes para PvP, safe zone, dungeon, trigger e mapa adjacente.
- Paths para patrulha de mobs.

## Formato inicial recomendado

JSON deterministico com `schema_version`, `map_id`, `coordinate_system`, `bounds` e arrays ordenados por id.

Campos recomendados:

- `colliders`: id, shape, position, rotation, size ou polygon.
- `spawns`: id, kind, position, rotation, tags.
- `mob_camps`: id, mob_table, spawn_positions, leash_radius, respawn_seconds.
- `transitions`: id, from_map, to_map, volume, target_spawn_id.
- `zones`: id, kind, volume, flags.
- `nav`: nodes/edges ou referencia de arquivo navmesh simplificado.

## Validacoes

- Id unico por mapa.
- Spawn dentro de bounds e fora de collider bloqueado.
- Transition aponta para mapa e spawn existentes.
- Mob camp tem pelo menos um spawn.
- Shapes suportados pelo loader Rust.
- Versao de schema aceita pelo servidor.

## Paths sugeridos

- Export Godot: `client/exported_world/<map_id>.world.json`
- Consumo Rust: `server/assets/world/<map_id>.world.json`
- Validador: `skills/wm-world-pipeline/scripts/validate_world_export.py`
