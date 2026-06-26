# Persistence/Auth - contexto

Arquivos principais:

- `server/crates/persistence/src/lib.rs`
- `server/crates/persistence/src/main.rs`
- `server/crates/services/auth/src/lib.rs`
- `docker/migrations/001_initial.sql`
- `docker/compose.yml`
- `proto/game_messages.proto`

## Auth atual

- `SecurityConfig::from_env` exige `JWT_SECRET` e tamanho minimo 32.
- JWT expira por default em 900s.
- Registro insere em `players`.
- Login busca credenciais, verifica Argon2 em `spawn_blocking`, gera JWT e salva sessao no Redis.
- Rate limit: 5 tentativas por minuto por IP.

## Persistence worker atual

- Usa `async_nats::jetstream`.
- Stream `PERSISTENCE` com subjects `persistence.>`.
- Consumer duravel `db-sync` com ACK explicito.
- Subjects conhecidos:
  - `persistence.snapshot`
  - `persistence.event.levelup`
  - `persistence.event.loot`

## Schema mental

- `players`: conta e hash de senha.
- `player_characters`: personagem, nivel, XP, HP e posicao.
- `player_inventory`: slots por personagem com `item_data` JSONB.

## Regras duras

- Redis e cache/sessao/ranking, nao fonte de verdade de inventario.
- PostgreSQL e fonte ACID para economia.
- Eventos criticos nao podem depender apenas de snapshot de 30s.
- Handlers que fazem UPSERT devem tolerar reprocessamento por at-least-once delivery.
