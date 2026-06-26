---
name: wm-architecture-roadmap
description: Direcao tecnica e roadmap jogavel do MMORPG Weapons Masters. Use para planejar milestones, cortar escopo, escolher arquitetura 2026, alinhar Rust server + Godot client, decidir PC/Web/Mobile, sequenciar features, definir criterio de pronto e impedir feature creep antes de cada step jogavel.
---

# Weapons Masters Architecture Roadmap

Use esta skill para manter o projeto com rumo. A prioridade nao e fazer muito; e fazer a fundacao certa na ordem certa.

## Workflow

1. Ler `references/project-context.md` para tarefas de roadmap, arquitetura ou escopo.
2. Ler `../wm-ai-engineering-standards/references/source-notes-2026.md` quando a decisao tocar Web, ECS ou JetStream.
3. Identificar o step real pelo codigo atual, nao pelo desejo do roadmap.
4. Quebrar a entrega em fatia vertical: contrato, servidor, cliente, teste, observabilidade minima.
5. Definir criterio de pronto jogavel: o que abrir, qual comando rodar, qual comportamento ver.

## Norte Tecnico 2026

- Servidor Rust autoritativo com ECS para World Server.
- Gateway persistente entre cliente e World Server.
- Protobuf como contrato unico entre Rust e C#.
- PostgreSQL como fonte ACID de economia e personagens.
- NATS JetStream para eventos duraveis e persistencia assicrona.
- Redis apenas para sessao, cache e ranking.
- Observabilidade desde cedo, nao depois do alpha.

## Decisao Critica: Web

Godot 4 C# atualmente nao tem export Web oficial. Sempre que o usuario pedir PC + Web:

- Nao prometer um unico cliente Godot C# exportado para browser.
- Oferecer alternativas: cliente nativo C# primeiro, cliente Web separado, camada Web em GDScript, ou esperar suporte oficial.
- Se Web for obrigatorio no curto prazo, tratar como arquitetura separada e testar cedo.

## Ordem Dos Steps

1. Rede jogavel e prediction basica.
2. Combate autoritativo com dodge, LoS, cooldown e lag compensation.
3. Mobs, XP, inventario, auth e persistencia.
4. Social, PvP, trade, mapas multiplos, anti-cheat e observabilidade.
5. Alpha publico com classes, dungeons, party, CDN, CI/CD e cloud.

## Corte De Escopo

- Se uma feature nao melhora o step atual, mover para backlog.
- Se uma feature exige economia, adicionar persistencia ACID antes.
- Se uma feature exige Web, validar limitacao Godot C# antes.
- Se uma feature cria abuso, acionar `$wm-anticheat-security`.
- Se uma feature mexe progressao, acionar `$wm-game-design-economy`.
