---
name: wm-godot-client
description: Cliente Godot 4 C# do Weapons Masters. Use para scenes, scripts C#, input, camera, UI/HUD, login, inventario, HP/XP bars, feedback visual, Protobuf, WebSocket/WebTransport client, prediction, reconciliation, interpolation, performance de frame e limitacoes de export Web em Godot C#.
---

# Weapons Masters Godot Client

Use esta skill para construir um cliente responsivo sem roubar autoridade do servidor.

## Workflow

1. Ler `references/godot-client-context.md`.
2. Conferir `proto/game_messages.proto` antes de criar ou alterar mensagens.
3. Separar captura de input, rede, prediction, render e UI.
4. Testar no editor; quando tocar plataforma, validar target real.
5. Se a tarefa mencionar browser, avisar que Godot 4 C# nao exporta oficialmente para Web.

## Autoridade

- Predizer movimento local e feedback reversivel.
- Nao predizer dano final, morte, XP, loot, inventario, trade ou cooldown autoritativo.
- Aplicar snapshot server-side como verdade.
- Usar `local_entity_id` para distinguir player local.

## Performance C#

- Evitar LINQ, closures e alocacoes em `_Process`/`_PhysicsProcess`.
- Reutilizar buffers, nodes de UI e objetos de efeito.
- Cachear NodePaths e referencias no `_Ready`.
- Separar parsing de snapshot de atualizacao visual quando o payload crescer.

## Reconciliation

- Cada input local tem `sequence`.
- Ao snapshot chegar, remover inputs confirmados.
- Reaplicar pendentes somente para movimento local.
- Interpolar entidades remotas; teleportar apenas em correcao grande, spawn ou troca de mapa.

## UI

- UI reflete snapshots e eventos.
- Login nao loga senha nem token.
- Erros de rede precisam ser visiveis e recuperaveis.
- Inventario e economia devem esperar confirmacao do servidor.
