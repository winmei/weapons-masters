# Weapons Masters - contexto de arquitetura

Projeto em `C:\Users\bruno\Desktop\weapons-masters`.

## Stack

- Server: Rust workspace com crates `gateway`, `world`, `shared`, `persistence` e `services/auth`.
- Client: Godot 4.7 C# em `client/`, com `Google.Protobuf` e `Grpc.Tools`.
- Contratos: `proto/game_messages.proto` e a fonte de verdade compartilhada entre Rust e C#.
- Dev infra: `docker/compose.yml`, PostgreSQL, Redis e NATS JetStream.

## Estado observado do codigo

- Gateway usa WebTransport/QUIC na porta UDP 4433 e WebSocket fallback na TCP 8080.
- Gateway atribui `entity_id` e sobrescreve qualquer identidade enviada pelo cliente.
- World Server roda game loop em thread dedicada a 30Hz com Bevy ECS.
- Existem sistemas de movimento, colisao, dodge, combate, mobs, XP, snapshots e eventos de combate.
- Auth usa Argon2, JWT obrigatorio via `JWT_SECRET`, Redis para rate limit/sessao por IP.
- Persistence consome NATS JetStream `PERSISTENCE` com sujeitos `persistence.snapshot`, `persistence.event.levelup` e `persistence.event.loot`.

## Guardrails

- Nao colocar I/O de banco, Redis ou NATS dentro de sistemas ECS do tick quente.
- Nao confiar em posicao, tempo, entidade, hit ou dano enviados pelo cliente.
- Nao duplicar definicoes de mensagens fora do `.proto`.
- Nao adicionar UI ou conteudo que dependa de estado inexistente no servidor.
- Preservar testes unitarios de funcoes puras quando mexer em regras de gameplay.
