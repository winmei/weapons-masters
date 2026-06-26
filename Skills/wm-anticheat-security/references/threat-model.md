# Threat model - Weapons Masters

## Ataques esperados

- Speed/teleport: cliente envia movimento acima da velocidade maxima.
- Cooldown bypass: cliente manda skill/dodge em frequencia impossivel.
- Lag switch: jogador manipula timing para ganhar hit/dodge.
- Replay: pacote antigo reaplicado.
- Packet edit: alvo, skill ou direcao adulterados.
- Dup: disconnect ou retry em trade/drop.
- Bot farm: caminho repetitivo, acoes 24/7, economia inflada.
- RMT: trades repetidos e desbalanceados entre contas relacionadas.

## Dados confiaveis

- Tempo do servidor.
- Identidade atribuida pelo Gateway/Auth.
- Estado em RAM do World Server.
- Transacoes confirmadas no PostgreSQL.

## Dados nao confiaveis

- Tudo que vem do cliente: timestamp, posicao final, hit, dano, alvo, inventario, gold, item, preco e cooldown.
