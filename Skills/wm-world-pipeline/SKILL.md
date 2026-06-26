---
name: wm-world-pipeline
description: Pipeline de mundo do Weapons Masters entre Godot e servidor Rust. Use para exportar mapas, colisores, navmesh, spawn points, mob camps, areas de transicao, zonas PvP, triggers, bounds, metadados de cena e ferramentas CLI/Rider/Godot que serializam dados visuais do cliente para dados fisicos/logicos consumidos pelo World Server bevy_ecs.
---

# Weapons Masters World Pipeline

Use esta skill para impedir o erro caro de manter dois mapas diferentes: um visual no Godot e outro logico no Rust. O mapa visual e a fonte de autoria; o servidor consome uma exportacao validada.

## Workflow

1. Ler `references/world-pipeline-contract.md`.
2. Identificar a fonte de autoria no Godot: scene, markers, groups, custom resources ou metadata.
3. Definir formato de export estavel: JSON para tooling inicial, Protobuf/binario quando o contrato amadurecer.
4. Gerar arquivo versionado para o servidor em `server` ou `assets/world`.
5. Validar no CLI: schema, coordenadas, duplicidade de ids, bounds, spawns e transicoes.
6. Carregar no startup do World Server sem abrir Godot em runtime.

## Dados Exportaveis

- Colisores server-side e areas bloqueadas.
- Navmesh ou grafo simplificado para mobs.
- Spawn points de players, mobs, bosses e recursos.
- Mob camps, patrol routes e leash zones.
- Areas de transicao entre mapas.
- Zonas PvP, safe zones, dungeons e triggers.
- Bounds do mapa e celulas de AOI.
- Pontos de interesse para teleporte, quests e eventos.

## Regras

- Nao duplicar coordenada manualmente em Rust se ela vem do mapa.
- Toda entidade exportada precisa de id estavel e tipo explicito.
- Export deve ser deterministico para gerar diff legivel.
- O servidor deve validar o arquivo e falhar cedo se o mapa estiver inconsistente.
- Cliente visual nunca decide colisao autoritativa; servidor usa export validado.
- Mudanca no formato requer versionamento e migracao de loader.

## Ferramentas

- Preferir script CLI que possa rodar pelo Rider, terminal ou CI.
- Se usar Godot headless, documentar comando e caminho do projeto.
- Para prototipo, aceitar JSON pretty-printed com schema simples.
- Para producao, considerar Protobuf ou formato binario com checksum.
- Incluir comando de validacao separado do comando de export.

## Integracao

- Acionar `$wm-server-gameplay` ao carregar dados no World Server.
- Acionar `$wm-godot-client` ao criar markers, editor plugins ou scene metadata.
- Acionar `$wm-network-protocol` se dados de mapa afetarem snapshots ou transicao.
- Acionar `$wm-infra-ops` se o pipeline entrar em CI/CD.
