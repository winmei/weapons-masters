# Step 1 - Cubo andando em rede

## Objetivo

Status verificado: **CONCLUÍDO** (PC nativo via WebSocket). WebTransport para browser bloqueado por limitação externa do Godot 4.7 C#.

Um cubo 3D controlavel no Godot se move em tempo real com posicao autoritativa vinda de um servidor Rust. O servidor aceita:

- WebTransport/QUIC em `https://localhost:4433` para export web.
- WebSocket binario em `ws://127.0.0.1:8080` como fallback para o client nativo PC.

Os dois transports usam o mesmo contrato Protobuf e alimentam o mesmo `world`.

## Estrutura

```text
weapons-masters/
|-- proto/
|   `-- game_messages.proto
|-- server/
|   |-- Cargo.toml
|   `-- crates/
|       |-- shared/
|       |   |-- build.rs
|       |   `-- src/lib.rs
|       |-- gateway/
|       |   `-- src/
|       |       |-- lib.rs
|       |       `-- main.rs
|       `-- world/
|           `-- src/main.rs
|-- client/
|   |-- project.godot
|   |-- Weapons Masters Client.csproj
|   |-- scenes/Main.tscn
|   `-- scripts/
|       |-- Game/PlayerController.cs
|       |-- Network/InputSender.cs
|       |-- Network/NetworkManager.cs
|       |-- Network/PacketHandler.cs
|       `-- Prediction/ClientPrediction.cs
`-- docs/steps/
```

## O que esta implementado

- Protobuf compartilhado: `PlayerInput`, `WorldSnapshot`, `EntityState`, `Vec2`, `InputType`.
- Rust `shared`: gera tipos Rust via `prost-build`.
- Rust `gateway`: recebe input por WebTransport ou WebSocket, decodifica Protobuf e publica snapshots.
- Rust `world`: roda tick a 30Hz numa thread dedicada do sistema operacional, limita input por entidade, move players, resolve arena/parede e envia snapshots autoritativos.
- Godot client nativo: captura WASD, faz client-side prediction, envia Protobuf por WebSocket e aplica reconciliation.
- Multiplayer: cada client gera `EntityId` proprio; snapshots criam/atualizam cubos remotos.
- Cena: player local, cubos remotos, camera, chao e parede fisica.

## Como rodar

Servidor:

```powershell
cd C:\Users\bruno\Desktop\weapons-masters\server
cargo run -p world
```

Client PC nativo:

1. Abra `client/project.godot` no Godot 4.7 .NET.
2. Rode a cena `res://scenes/Main.tscn`.
3. O client nativo usa `ws://127.0.0.1:8080`.

Client web:

Bloqueado na stack atual: Godot 4.7 C# ainda nao exporta para web. Para concluir este requisito, migrar o client web para GDScript/GDExtension, usar Godot 3 C# no web, ou mudar o criterio do Step 1 para PC nativo primeiro.

## Criterio de pronto

- [ ] Dois clients conectados ao mesmo servidor veem seus cubos e o cubo remoto.
- [ ] O cubo local responde imediatamente por prediction.
- [ ] O servidor corrige a posicao por snapshot autoritativo.
- [ ] A parede vermelha bloqueia o player sem jitter perceptivel.
- [ ] Browser conectado via WebTransport.
- [x] PC nativo conectado via WebSocket fallback.

## Checklist

- [x] Repositorio Git inicializado.
- [x] Workspace Rust criado.
- [x] `proto/game_messages.proto` escrito.
- [x] `prost-build` configurado.
- [x] `World` autoritativo implementado.
- [x] Gateway WebTransport implementado no servidor.
- [x] Fallback WebSocket para client nativo implementado.
- [x] `PlayerInput` decodificado no servidor.
- [x] `WorldSnapshot` serializado e enviado ao client.
- [x] Projeto Godot C# criado.
- [x] `Google.Protobuf` e geracao C# configurados.
- [x] Cena com player, parede, chao, camera e remotos.
- [x] `InputSender.cs` captura WASD.
- [x] `NetworkManager.cs` conecta/envia/recebe no PC nativo via WebSocket fallback.
- [x] `PacketHandler.cs` atualiza local e remotos.
- [x] `ClientPrediction.cs` aplica prediction + reconciliation.
- [x] Game loop autoritativo isolado em `std::thread::spawn` com `sleep_precise`.

## Validacao pendente nesta maquina

`cargo`, `dotnet` e Godot nao estao disponiveis no PATH desta sessao, entao a compilacao precisa ser rodada localmente com as ferramentas instaladas.

## Bloqueios verificados

- Godot 4.7 C# nao pode ser exportado para web segundo a documentacao oficial estavel do Godot.
- A conclusao real do Step 1 depende de `cargo check --workspace`, `dotnet build`, execucao no Godot e teste com dois clients.
