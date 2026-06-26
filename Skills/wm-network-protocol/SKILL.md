---
name: wm-network-protocol
description: Netcode, Gateway e contratos Protobuf do Weapons Masters. Use para `proto/game_messages.proto`, WebTransport, WebSocket fallback, Gateway Rust, snapshots, inputs, rate limit, identidade server-assigned, reconciliation, compatibilidade Rust/C#, payload binario, versionamento e seguranca de rede.
---

# Weapons Masters Network Protocol

Use esta skill quando uma mudanca atravessar cliente, Gateway ou World Server. O protocolo e parte da seguranca.

## Workflow

1. Ler `references/network-context.md`.
2. Alterar `.proto` primeiro e preservar numeros de campos existentes.
3. Atualizar geracao/consumo em Rust e C#.
4. Validar encode/decode e defaults.
5. Se tocar Gateway/infra, conferir exposicao UDP do WebTransport e TCP do fallback.
6. Testar conexao ou adicionar teste de contrato.

## Regras Protobuf

- Nunca renumerar campo publicado.
- Campos removidos devem entrar em `reserved` com numero e nome.
- Preferir mensagens explicitas a JSON em gameplay.
- Defaults precisam ser seguros: `0` nao pode significar privilegio.
- Eventos novos em `oneof` exigem handler cliente e fallback seguro.
- Mudanca de contrato deve atualizar Rust, C#, testes de encode/decode e documentacao da mensagem.
- Ao adicionar enum, tratar valor desconhecido no servidor e no cliente.
- Nao reutilizar campo de outra semantica, mesmo em prototipo.

## Gateway

- Atribuir e manter `entity_id` server-side.
- Aplicar rate limit por sessao e tamanho maximo de payload.
- Rejeitar decode invalido sem panic.
- Personalizar `local_entity_id` por conexao.
- Nao guardar estado autoritativo de gameplay no Gateway.
- Implementar keep-alive/heartbeat ou timeout de inatividade para WebTransport e WebSocket; conexoes half-open nao podem manter entidade viva indefinidamente.
- Ao detectar timeout, sinalizar disconnect para o World Server ou parar de renovar atividade da entidade de forma previsivel.

## Transportes

- WebTransport/QUIC e bom alvo para browser moderno.
- WebTransport usa QUIC sobre UDP; Docker, Kubernetes, firewall e load balancer precisam expor UDP explicitamente, por exemplo `4433/udp`.
- WebSocket binario e fallback, com custo maior e sem semantica UDP-like.
- WebSocket fallback usa TCP separado, por exemplo `8080/tcp`.
- TCP/WebSocket e QUIC podem ficar half-open quando a rede cai sem close frame; nunca depender apenas de evento de fechamento.
- PC/mobile nativo podem evoluir para UDP/KCP/QUIC se o cliente suportar.
- Godot 4 C# nao deve ser assumido como cliente Web exportavel; planejar alternativa.

## Disconnect E Reconnect

- Definir heartbeat/keep-alive por transporte e timeout server-side explicito.
- Remover ou marcar entidades inativas no World Server apos timeout curto para prototipo e regra configuravel para producao.
- Preservar estado suficiente para reconnect sem duplicar entidade.
- Distinguir disconnect normal, timeout half-open e kick por rate limit nos logs.
- Medir conexoes half-open, timeouts, reconnects e entidades removidas por inatividade.

## Checklist De Contrato

- O campo novo tem numero unico e nome estavel?
- O default e seguro para clientes antigos?
- O servidor rejeita payload invalido sem panic?
- O cliente ignora evento desconhecido sem quebrar render?
- Existe teste de round-trip Protobuf ou smoke real?
- A mudanca exige migration de save ou compatibilidade de snapshot?
- O comportamento de timeout/reconnect foi definido para este pacote ou estado?

## Snapshot Design

- Snapshot global simples serve no prototipo.
- Para escala, evoluir para AOI, delta compression e prioridade por relevancia.
- Medir tamanho medio, bytes/s e lagged receivers.
- Nao enviar drop tables, seeds sensiveis ou dados economicos desnecessarios.
