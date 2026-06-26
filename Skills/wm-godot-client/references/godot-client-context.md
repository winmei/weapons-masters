# Godot Client - contexto

Arquivos principais:

- `client/project.godot`
- `client/Weapons Masters Client.csproj`
- `client/scenes/Main.tscn`
- `client/scenes/Login.tscn`
- `client/scripts/Network/NetworkManager.cs`
- `client/scripts/Network/InputSender.cs`
- `client/scripts/Network/PacketHandler.cs`
- `client/scripts/Prediction/ClientPrediction.cs`
- `client/scripts/Game/PlayerController.cs`
- `client/scripts/UI/LoginScreen.cs`

## Stack C#

O projeto usa `Godot.NET.Sdk/4.7.0`, `net8.0`, `Google.Protobuf` e `Grpc.Tools`. O `.csproj` inclui `..\proto\game_messages.proto` com `GrpcServices="None"`.

## Contrato mental

- Godot captura input e envia `PlayerInput`.
- Servidor responde `WorldSnapshot`.
- O cliente usa `local_entity_id` para identificar o jogador local.
- Prediction melhora sensacao de movimento, mas reconciliation precisa aceitar correcao do servidor.

## Cuidados

- Evitar criar mensagens Protobuf manualmente fora das classes geradas.
- Evitar LINQ e alocacoes em loops por frame.
- Separar rede, prediction e visual: `NetworkManager`, `InputSender`, `PacketHandler`, `ClientPrediction`, `PlayerController`.
- Quando adicionar UI, conectar a eventos de snapshot em vez de buscar estado global solto.
