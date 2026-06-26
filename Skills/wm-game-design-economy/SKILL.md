---
name: wm-game-design-economy
description: Game design sistemico para Weapons Masters. Use para classes, skills, combate tab-target, dodge, progression, XP curves, mobs, loot tables, itemization, inventory, gold sources/sinks, trade, party, dungeon rewards, balanceamento, retencao e economia saudavel de MMORPG.
---

# Weapons Masters Game Design Economy

Use esta skill para desenhar sistemas divertidos que tambem sao implementaveis, testaveis e seguros.

## Principio

Primeiro validar feel e regra, depois conteudo. Um bom MMORPG nasce de loops pequenos, claros e repetiveis: mover, mirar, lutar, sobreviver, ganhar, equipar, voltar.

## Workflow

1. Definir o loop de 30 segundos, 5 minutos e 1 hora.
2. Definir quais decisoes sao do jogador e quais sao calculadas pelo servidor.
3. Especificar numeros iniciais simples e telemetria para balancear depois.
4. Criar tabela ou config versionavel, nao constante espalhada.
5. Exigir teste de borda: nivel maximo, XP overflow, drop raro, item stack, morte e reconnect.

## Combate

- Tab-target com dodge precisa de leitura clara: cast, telegraph, i-frame, cooldown e feedback.
- Skills devem ter custo, janela de contra-jogo e papel.
- Dano final server-side.
- Balancear por tempo para matar, risco, alcance, mobilidade e cooldown.

## Progressao

- Curva de XP deve ser monotona, previsivel e ajustavel.
- Level up deve dar recompensa perceptivel, mas nao quebrar PvP.
- Mobs precisam de papel: fraco comum, elite, boss, dungeon.
- Rewards devem incentivar grupo sem virar farm infinito.

## Economia

- Toda source precisa de sink.
- Gold sink inicial: repair, teleport, enhance, auction tax, crafting fee.
- Drops raros precisam de instancia unica e auditoria.
- Trade exige transacao ACID e logs economicos.
- Medir inflacao: gold total, gold generated/removed, itens raros ativos, trades suspeitos.
