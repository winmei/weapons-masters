---
name: wm-liveops-moderation
description: LiveOps, Game Master tools, moderacao e operacoes ao vivo para Weapons Masters. Use para comandos administrativos, paineis GM, mute/ban/chat moderation, restituicao segura de itens, eventos de bonus XP/drop, feature flags, auditoria, RBAC, suporte ao jogador, rollback operacional e isolamento de privilegios.
---

# Weapons Masters LiveOps Moderation

Use esta skill para planejar ferramentas que mantem o MMORPG vivo sem transformar administracao em exploit. Poder de GM e codigo de producao com luvas de amianto.

## Workflow

1. Ler `references/liveops-contract.md`.
2. Definir quem pode executar a acao: suporte, GM, admin, sistema ou deploy.
3. Exigir RBAC, auditoria e motivo humano para toda acao privilegiada.
4. Separar comandos administrativos da logica comum de jogador.
5. Para economia, usar transacao e evento auditavel.
6. Para eventos ao vivo, preferir feature flag/config dinamica com expiracao.

## Ferramentas GM

- Mute/unmute por chat global, mapa, whisper ou conta.
- Kick/ban/suspensao com duracao e motivo.
- Teleport de GM invisivel ou modo observador.
- Consulta de inventario, logs de trade, posicao e historico de login.
- Restituicao de item/gold com ticket, aprovacao e transacao.
- Broadcast de aviso e eventos ao vivo.

## Regras De Seguranca

- GM nunca usa endpoint de jogador com parametros extras.
- Toda acao privilegiada grava actor, target, motivo, timestamp, diff e correlation id.
- Restituicao de item cria instancia nova auditavel; nao edita inventario silenciosamente.
- Feature flag tem escopo, duracao, owner e rollback.
- Comando administrativo precisa ser idempotente quando puder ser repetido.
- Nunca expor painel GM sem auth forte, RBAC e logs.

## Live Events

- Bonus XP/drop deve ser configuravel por mapa, janela de tempo e multiplicador maximo.
- Evento precisa de preview e dry-run quando afetar economia.
- Expiracao automatica obrigatoria para flag temporaria.
- Metricas: jogadores impactados, XP extra gerado, gold extra, drop raro extra e erro por comando.

## Integracao

- Acionar `$wm-persistence-auth` para restituicao, inventario, migrations e auditoria.
- Acionar `$wm-anticheat-security` para abuso social, ban, RMT e permissao.
- Acionar `$wm-infra-ops` para painel, secrets, logs e dashboards.
- Acionar `$wm-game-design-economy` para eventos que mexem XP, loot ou gold.
