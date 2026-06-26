---
name: wm-persistence-auth
description: Persistencia, auth, sessoes e economia ACID do Weapons Masters. Use para PostgreSQL, Redis, NATS JetStream, DB worker, migrations, Argon2, JWT, rate limit, player saves, inventory, loot, level-up, trade, idempotencia, ACK explicito, backups e seguranca de dados.
---

# Weapons Masters Persistence Auth

Use esta skill para qualquer dado que precisa sobreviver a crash, reconnect ou tentativa de abuso.

## Workflow

1. Ler `references/persistence-auth-context.md`.
2. Classificar dado: quente de gameplay, evento critico ou estado permanente.
3. Usar PostgreSQL para fonte de verdade e Redis apenas para cache/sessao.
4. Usar JetStream para persistencia assicrona com ACK explicito.
5. Garantir idempotencia ou dedupe antes de aceitar at-least-once.

## Auth

- `JWT_SECRET` obrigatorio, forte e externo.
- Senha com Argon2.
- Verificacao de senha fora do event loop.
- Rate limit por IP e, quando existir conta, por usuario.
- Token nunca em log.
- Refresh token deve ser revogavel e armazenado como hash.

## Persistencia

- Snapshot periodico serve para estado tolerante a pequena perda: posicao, HP, progresso comum.
- Eventos criticos sao imediatos: trade, drop raro, compra, venda, upgrade, cash.
- ACK de JetStream somente apos escrita bem sucedida ou decisao terminal registrada.
- Handlers devem tolerar reentrega.
- Migrations precisam preservar dados existentes.

## Economia ACID

- Trade e inventario mudam em uma transacao.
- Itens precisam de instancia unica quando forem equipaveis/raros.
- Gold sink/source deve gerar evento auditavel.
- Nunca aceitar quantidade, item id raro ou preco final decidido pelo cliente.
