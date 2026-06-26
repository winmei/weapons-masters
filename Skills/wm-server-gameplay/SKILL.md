---
name: wm-server-gameplay
description: Gameplay autoritativo no World Server Rust do Weapons Masters. Use para `server/crates/world`, Bevy ECS, tick loop 30Hz, movimento, colisao, combate tab-target, dodge/i-frames, lag compensation, mobs, XP, loot, AOI, snapshots, determinismo, performance e regras server-side de MMORPG.
---

# Weapons Masters Server Gameplay

Use esta skill para escrever o codigo que decide a verdade do mundo. O servidor deve ser injusto com cheats e justo com jogadores sob lag normal.

## Workflow

1. Ler `references/world-server-context.md`.
2. Localizar a posicao correta no schedule ECS antes de editar.
3. Manter regra de dominio testavel fora de transporte, DB e UI.
4. Medir ou estimar custo por tick antes de adicionar loops.
5. Rodar `cargo test` no servidor e adicionar teste para regra nova.

## Invariantes

- Tick quente nao faz I/O, lock pesado, sleep, DNS, HTTP, SQL, Redis ou NATS.
- Cliente envia intent; servidor decide resultado.
- Tempo confiavel vem do servidor.
- Estado de combate e economia nao depende do Gateway.
- Cada regra critica tem teste unitario ou ECS isolado.
- Entidade de jogador precisa expirar por inatividade mesmo se a conexao nao emitir close frame.

## Padroes ECS

- Componentes: dados pequenos por entidade.
- Recursos: indices, filas, caches e configuracao global.
- Sistemas: uma responsabilidade e ordem explicita quando houver dependencia.
- Queries: evitar O(N^2); usar spatial hash/AOI para proximidade.
- Eventos internos: drenar uma vez, aplicar em ordem previsivel, limpar no fim do tick.

## Combate

- Validar atacante vivo, alvo vivo, range, LoS, cooldown, cast, i-frame e estado.
- Limitar rewind de lag compensation; nunca aceitar timestamp do cliente.
- Separar `check_hit`/calculo de dano em funcoes puras.
- Snapshot informa eventos; cliente nao calcula dano final.

## Mobs, XP E Loot

- IA como maquina de estados clara.
- Respawn e morte server-side.
- XP e loot sao eventos de dominio, depois persistidos fora do tick.
- Loot raro deve ter caminho duravel imediato quando virar economia real.

## Performance

- Reutilizar buffers de snapshot.
- Evitar clone grande por conexao quando Arc, delta ou AOI resolver.
- Adicionar metricas se o sistema novo roda todo tick.
- Preferir dados contiguos e iteracoes previsiveis a abstracoes dinamicas no hot path.

## Desconexao E Entidade Fantasma

- Atualizar `LastActive` apenas quando input/heartbeat valido chega do jogador.
- Remover, congelar ou marcar como desconectada a entidade apos timeout configurado.
- Nao permitir que jogador half-open continue ocupando combate, trade, dungeon ou party sem regra explicita.
- Em PvP, tratar disconnect com janela curta de protecao ou permanencia controlada, nao despawn instantaneo exploravel.
- Testar queda abrupta de rede, reconnect e timeout sem close frame.
