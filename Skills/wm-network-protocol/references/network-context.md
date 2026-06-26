# Network Protocol - contexto

Arquivos principais:

- `proto/game_messages.proto`
- `server/crates/gateway/src/lib.rs`
- `server/crates/shared/build.rs`
- `client/Weapons Masters Client.csproj`
- `client/scripts/Network/NetworkManager.cs`
- `client/scripts/Network/InputSender.cs`
- `client/scripts/Network/PacketHandler.cs`

## Mensagens atuais

- `PlayerInput`: `sequence`, `input_type`, `direction`, `skill_use`, `dodge`.
- `WorldSnapshot`: `tick`, `last_processed_input`, `entities`, `combat_events`, `local_entity_id`.
- `EntityState`: posicao, rotacao, flags de colisao, HP, action e ultimo input processado.
- `CombatEvent`: `DamageEvent`, `DodgeResult`, `DeathEvent`.
- Auth e persistencia tambem estao no mesmo `.proto`: `LoginRequest`, `LoginResponse`, `CharacterData`, `InventorySlot`, `ItemData`.

## Gateway atual

- `run_gateway` inicia WebSocket fallback e WebTransport.
- WebTransport usa certificado self-signed para localhost.
- WebTransport/QUIC escuta UDP 4433 por padrao (`DEFAULT_BIND_PORT`).
- WebSocket fallback escuta TCP 8080 por padrao (`DEFAULT_WEBSOCKET_PORT`).
- WebTransport recebe datagramas e envia datagramas.
- WebSocket usa frames binarios.
- `assigned_entity_id` e calculado no Gateway e sobrescreve qualquer valor vindo do cliente.
- Snapshot e clonado por conexao para preencher `local_entity_id`.

## Infra minima do Gateway

- Docker Compose deve publicar `4433:4433/udp` para WebTransport.
- Docker Compose deve publicar `8080:8080/tcp` para WebSocket fallback.
- Kubernetes deve separar `protocol: UDP` e `protocol: TCP` no Service.
- Firewalls e security groups precisam liberar UDP 4433 alem de TCP 8080.
- Testar conexao de fora do container; porta TCP aberta nao valida QUIC.

## Half-open e timeout

- WebTransport e WebSocket podem nao entregar fechamento imediato quando a rede do cliente cai.
- Gateway/World Server precisam de heartbeat, keep-alive ou timeout por ultima atividade.
- O codigo atual do World Server ja possui timeout de inatividade de player; manter essa regra visivel ao evoluir reconnect.
- Timeouts devem limpar entidade/sessao sem depender exclusivamente de close frame.
- Reconnect futuro deve evitar entidade duplicada: a sessao antiga expira ou e substituida de modo explicito.

## Cuidados ao mudar protocolo

- Se adicionar input novo, atualizar enum `InputType`, mensagem associada e `apply_input_to_intent`.
- Se adicionar evento novo, atualizar `CombatEvent oneof`, cliente `PacketHandler` e VFX/UI correspondente.
- Se adicionar estado visual em `EntityState`, garantir default seguro para entidades antigas.
- Se aumentar snapshots, avaliar delta, AOI ou compressao antes de transmitir listas enormes.
- Se remover campo, reservar numero e nome no `.proto`.
- Se adicionar enum, validar comportamento para valores desconhecidos.
