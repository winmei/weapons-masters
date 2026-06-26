---
name: wm-anticheat-security
description: Anti-cheat e seguranca para Weapons Masters. Use para prevenir speed hack, teleport, input spam, cooldown bypass, dodge abuse, lag switch, packet replay, item dup, trade exploit, bot farm, RMT, auth abuse, token leakage, rate limiting, auditoria e deteccao server-side em MMORPG.
---

# Weapons Masters Anti-Cheat Security

Use esta skill para perguntar: "como um jogador malicioso quebraria isso?". Toda feature multiplayer precisa dessa resposta.

## Modelo De Ameaca

- Cliente e hostil.
- Rede e instavel e manipulavel.
- Jogadores tentam replay, macro, speed hack, lag switch, packet edit e disconnect exploit.
- Economia sera atacada por dup, bot farm, trade abuse e RMT.
- Logs tambem podem vazar segredo.

## Camadas

1. Prevencao autoritativa: servidor valida tudo.
2. Rate limit: input, login, chat, trade, skill, dodge e reconnect.
3. Deteccao online: velocidade impossivel, cooldown impossivel, posicao invalida, packet burst.
4. Deteccao offline: DPS impossivel, farm pattern, RMT graph, gold imbalance.
5. Resposta: reject, correct, flag, shadow-ban, disconnect ou manual review.

## Checklist Por Feature

- Qual dado o cliente pode falsificar?
- Qual e o limite fisico ou economico server-side?
- O que acontece se o pacote repetir?
- O que acontece se o jogador desconectar no meio?
- O que acontece se a conexao ficar half-open sem close frame?
- O que acontece se chegar fora de ordem?
- Qual metrica/log detecta abuso?

## Regras Duras

- Nao confiar em timestamp, posicao, alvo, dano, drop, item ou preco do cliente.
- Nunca criar item sem evento auditavel.
- Nunca transferir item sem transacao.
- Nunca punir automaticamente caso ambigo de lag sem evidencia adicional.
- Sempre distinguir correcao de estado normal de flag de cheat.
- Nunca deixar sessao half-open manter personagem, trade, party ou instancia preso indefinidamente.

## Disconnect Exploits

- Tratar timeout de inatividade como caso normal de rede, nao como prova automatica de cheat.
- Em trade/economia, disconnect deve abortar ou completar transacao de forma atomica, nunca deixar estado intermediario.
- Em combate/PvP, definir regra contra quit-to-avoid-death: permanencia temporaria, invulnerabilidade limitada ou penalidade clara.
- Registrar motivo: close normal, timeout heartbeat, kick rate limit, auth failure ou reconnect takeover.
