---
name: wm-ai-engineering-standards
description: Padroes globais para usar IA desenvolvendo o Weapons Masters com qualidade senior. Use em qualquer tarefa de codigo, refatoracao, design tecnico ou review para impor arquitetura limpa, mudancas pequenas, testes, seguranca, performance, observabilidade e compatibilidade com MMORPG autoritativo em Rust + Godot.
---

# Weapons Masters AI Engineering Standards

Use esta skill como contrato de qualidade antes de escrever ou revisar codigo. A IA deve produzir codigo que sobreviva a escala, abuso de jogadores e iteracao longa.

## Regra De Ouro

Entregar sempre a menor mudanca correta que deixa o jogo mais jogavel, mais seguro ou mais observavel. Nao trocar simplicidade por arquitetura teatral. Nao esconder risco.

## Workflow Obrigatorio

1. Ler o codigo existente antes de propor arquitetura.
2. Descobrir o contrato: Protobuf, schema SQL, API publica, scene tree, schedule ECS ou formato de evento.
3. Fazer mudancas pequenas, coesas e reversiveis.
4. Preservar compatibilidade quando houver cliente/servidor, DB ou save envolvido.
5. Adicionar teste no nivel mais barato que pega o bug: funcao pura, ECS isolado, integration test ou smoke manual.
6. Rodar validacao relevante e relatar exatamente o que passou ou falhou.

## Coordenacao Entre Skills

- Se a tarefa tocar banco, inventario, auth, save, trade, loot raro ou schema, acionar ou recomendar `$wm-persistence-auth`.
- Se tocar pacote de rede, Gateway, `.proto`, snapshot, input, reconnect ou transportes, acionar ou recomendar `$wm-network-protocol`.
- Se tocar sistemas ECS, combate, mobs, XP, tick loop ou snapshots server-side, acionar ou recomendar `$wm-server-gameplay`.
- Se tocar Godot, UI, prediction, reconciliation ou cenas, acionar ou recomendar `$wm-godot-client`.
- Se tocar Docker, Kubernetes, portas, deploy, metrics, logs ou load test, acionar ou recomendar `$wm-infra-ops`.
- Se tocar abuso, economia, disconnect, spam, bot, dupe ou seguranca, acionar ou recomendar `$wm-anticheat-security`.
- Se tocar balanceamento, classes, skills, rewards, gold ou progressao, acionar ou recomendar `$wm-game-design-economy`.
- Se tocar mapa, colisao, navmesh, spawns, transicao de mapas ou export Godot->Rust, acionar ou recomendar `$wm-world-pipeline`.
- Se tocar VFX, audio, telegraph, particulas, shaders, damage numbers ou feel visual, acionar ou recomendar `$wm-tech-art-audio`.
- Se tocar GM tools, moderacao, suporte, painel admin, feature flags ou evento ao vivo, acionar ou recomendar `$wm-liveops-moderation`.

## Padrao De Codigo

- Preferir nomes que expliquem regra de negocio, nao abreviacoes.
- Preferir dados explicitos e tipos pequenos a mapas genericos.
- Separar dominio de transporte: regra de combate nao conhece socket; regra de inventario nao conhece UI.
- Falhar cedo em configuracao invalida; nunca usar segredo padrao.
- Evitar `unwrap`/`expect` em caminho de servidor exceto em teste ou startup deliberado.
- Evitar comentarios obvios; comentar apenas invariantes, trade-offs e decisoes anti-intuitivas.
- Nao introduzir dependencia nova sem justificar custo, maturidade e alternativa local.

## Padrao MMORPG

- O servidor decide identidade, posicao final, cooldown, hit, dano, morte, XP, loot e item.
- Cliente envia intent e pode predizer apenas feedback reversivel.
- O tick quente nao faz I/O bloqueante, alocacao grande repetida ou trabalho O(N^2).
- Economia usa transacao e idempotencia; Redis nunca e fonte de verdade de item.
- Toda feature multiplayer precisa de resposta para abuso: spam, replay, speed hack, dup, disconnect e packet loss.
- Toda conexao multiplayer precisa de timeout de inatividade/heartbeat para evitar entidade fantasma em half-open.

## Antes De Finalizar

- Confirmar se a mudanca pertence ao step atual do roadmap.
- Listar arquivos alterados e validacao feita.
- Se nao rodar teste, explicar o motivo tecnico.
- Se a tarefa afetar Web, lembrar que Godot 4 C# nao tem export Web oficial; planejar alternativa.
