# LiveOps contract

## Principios

- Toda acao GM e evento de producao.
- Privilegio minimo por papel.
- Auditoria antes de conveniencia.
- Operacao temporaria precisa de expiracao.

## RBAC inicial

- `support`: consultar conta/personagem e criar ticket interno.
- `moderator`: mute, unmute, kick e chat review.
- `gm`: teleport assistido, observacao, eventos menores e suporte in-game.
- `economy_admin`: restituicao de item/gold com aprovacao.
- `admin`: feature flags globais, ban permanente e rollback operacional.

## Auditoria minima

Campos:

- `audit_id`
- `actor_account_id`
- `actor_role`
- `target_account_id`
- `target_character_id`
- `action`
- `reason`
- `before`
- `after`
- `created_at`
- `correlation_id`

## Restituicao segura

- Exigir ticket ou motivo.
- Validar item contra catalogo server-side.
- Criar instancia unica para item nao-stackavel.
- Inserir via transacao.
- Registrar diff do inventario.
- Emitir evento para observabilidade/economia.

## Feature flags

- Nome, escopo, valor, owner, inicio, fim, motivo e rollback.
- Flags de bonus economico devem ter limite maximo.
- Nunca deixar evento temporario sem `ends_at`.
