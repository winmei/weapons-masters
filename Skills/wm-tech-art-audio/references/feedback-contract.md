# Feedback contract

## Fontes autoritativas

- `DamageEvent`: numero de dano, flash, som de impacto, hit stop visual leve.
- `DodgeResult`: feedback de sucesso/falha de dodge.
- `DeathEvent`: morte, fade, ragdoll fake ou dissolve.
- `LevelUpEvent`: VFX/audio de progressao.
- `LootDrop`: brilho, som e UI de recompensa confirmada.

## Predicao permitida

- Movimento local.
- Inicio de cast local quando input e enviado.
- Telegraph cosmetico de skill propria.
- Animacao de dodge local.

## Confirmacao obrigatoria

- Dano aplicado.
- Morte.
- XP.
- Loot.
- Cooldown definitivo se houver divergencia.
- Buff/debuff com efeito mecanico.

## Budget inicial

- Efeito comum: baixo custo, poolado, sem shader novo em combate.
- Efeito raro/boss: pode ser mais caro, mas precisa LOD e limite de instancias.
- Damage numbers: pool e merge opcional quando eventos chegam em burst.
