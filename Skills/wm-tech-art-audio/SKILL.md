---
name: wm-tech-art-audio
description: Arte tecnica, VFX, audio e feedback de combate para Weapons Masters. Use para telegraphs, particulas, shaders, hit feedback, dodge feedback, audio cues, pooling, shader warmup, performance visual em Godot, resposta a CombatEvent server-side, readability de skills e polish sem stutter.
---

# Weapons Masters Tech Art Audio

Use esta skill para fazer o combate parecer bom sem mentir sobre o estado autoritativo. O servidor decide; o cliente comunica.

## Workflow

1. Ler `references/feedback-contract.md`.
2. Identificar o evento autoritativo que dispara o feedback: damage, dodge, death, cast, level up ou loot.
3. Separar feedback preditivo reversivel de feedback confirmado pelo servidor.
4. Usar pooling para VFX/audio frequentes.
5. Preaquecer shaders, materiais e particulas antes do combate.
6. Testar framerate com multiplas entidades e snapshots consecutivos.

## Regras De Feedback

- Telegraph pode aparecer antes do hit, mas dano final so apos evento do servidor.
- Dodge local pode tocar animacao imediata, mas sucesso/invulnerabilidade vem do servidor.
- VFX nao pode alocar recurso pesado no primeiro hit em combate real.
- Audio de impacto confirmado deve seguir `DamageEvent`, nao input local.
- Feedback precisa ser legivel: cor, forma, timing e som comunicam risco.
- Efeitos de inimigo, player local, party e hostil devem ser distinguiveis.

## Performance Godot

- Pool para particulas, labels de dano, decals e audio players.
- Cache de materiais, shaders, packed scenes e node refs.
- Evitar instanciar cena complexa em `_Process` ou durante burst de snapshot.
- Preload/warmup de shader em loading, lobby ou inicio do mapa.
- Limitar overdraw de particulas em area lotada.
- Criar LOD visual para distancia e densidade de jogadores.

## Audio

- Usar categorias: UI, ambiente, skill, impacto, alerta, voz.
- Evitar sobreposicao infinita; aplicar voice limiting por tipo de som.
- Eventos importantes precisam de cue curto e reconhecivel.
- Audio cosmetico nunca altera gameplay.

## Integracao

- Acionar `$wm-godot-client` para scripts, scenes e UI.
- Acionar `$wm-game-design-economy` para leitura de skill e counterplay.
- Acionar `$wm-server-gameplay` se o feedback exigir evento server-side novo.
- Acionar `$wm-network-protocol` se precisar adicionar `CombatEvent` ou estado no snapshot.
