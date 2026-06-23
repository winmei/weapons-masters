# Step 5 — Alpha Público: Classes, Dungeons e Conteúdo (6-8 semanas)

## Objetivo

O jogo está pronto para convidar **100-200 jogadores externos** para um alpha test. Tem 3 classes jogáveis, 2 dungeons instanciadas, arte low-poly estilizada, economia com gold sinks e infraestrutura em cloud.

## Pré-requisito: Step 4 completo (10+ players com chat, PvP, 2 mapas, anti-cheat)

---

## Conteúdo de Gameplay

### 1. Classes (3 classes, 4 skills cada)

| Classe | Skill 1 | Skill 2 | Skill 3 | Skill 4 (Ultimate) |
|:---|:---|:---|:---|:---|
| **Warrior** | Slash (melee 3m, 40dmg) | Shield Bash (melee 3m, stun 1s) | Charge (dash 10m + hit) | Whirlwind (AoE 5m, 80dmg) |
| **Mage** | Fireball (range 15m, 60dmg) | Frost Nova (AoE 8m, slow 3s) | Blink (teleport 10m) | Meteor (AoE 10m, 150dmg, 2s cast) |
| **Archer** | Quick Shot (range 20m, 35dmg) | Poison Arrow (range 15m, DoT 5s) | Backflip (dodge + 5m backward) | Rain of Arrows (AoE 12m, 100dmg) |

```rust
// Cada classe é um conjunto de SkillDefs + base stats
struct ClassDef {
    id: ClassId,
    base_stats: Stats,
    skills: [SkillDef; 4],
    dodge_distance: f32,
    movement_speed: f32,
}
```

### 2. Dungeons Instanciadas

- **Dungeon = World Server temporário** que sobe sob demanda (Docker container)
- Party de 3-5 jogadores entra → instância criada → dungeon jogada → instância destruída
- Boss com mecânicas: AoE telegrafado (círculo vermelho no chão), fases de HP, enrage timer

```rust
// Orquestrador de instâncias
async fn create_dungeon_instance(dungeon_id: &str, party: Vec<EntityId>) -> DungeonInstance {
    // Em dev local: spawn de novo processo na mesma máquina
    // Em produção: cria novo pod via Agones API
    let instance = spawn_world_server(DungeonConfig::load(dungeon_id));
    for player in party {
        migrate_player_to_instance(player, instance.address);
    }
    instance
}
```

### 3. Sistema de Party

- Convite: Player A convida Player B → B aceita → party formada
- HP do grupo visível na UI lateral
- Loot sharing: round-robin ou free-for-all (configurável pelo líder)
- XP dividido igualmente entre membros

### 4. Gold Sinks

| Sink | Implementação |
|:---|:---|
| Reparo de equipamento | Após morte: 10% do valor do item em ouro |
| Taxa de leilão | 5% sobre o valor de venda (removido da economia) |
| Enhance/upgrade | Custo crescente: nível × 100 gold |
| Teleporte rápido | 50 gold por teleporte entre mapas |
| Poções de NPC | 20 gold cada (consumível) |

---

## Infraestrutura de Produção

### Deploy em Cloud (Hetzner ~$80-150/mês para alpha)

```
1x VPS CX32 (4 vCPU, 8GB RAM) — World Servers + Gateway
1x VPS CX22 (2 vCPU, 4GB RAM) — PostgreSQL + Redis + NATS
```

Usar Docker Compose em ambas (não precisa de K8s para alpha).

### CDN para Assets Web

```
1. Build Godot: split .pck por mapa (main.pck ~50MB + maps/*.pck)
2. Upload maps/*.pck para Cloudflare R2 (free tier: 10GB)
3. Cliente Web baixa main.pck → joga lobby
4. Background: baixa map_plains.pck, map_forest.pck enquanto joga
```

### CI/CD (GitHub Actions — Blue-Green Deploy)

> **Nota de design:** O `docker compose pull && up -d` anterior derrubava o processo ativo,
> desconectando todos os jogadores durante a atualização. A abordagem abaixo usa deploy
> blue-green com drain de conexões via HAProxy, garantindo zero-downtime.

```yaml
# .github/workflows/deploy.yml
on:
  push:
    branches: [main]
jobs:
  build-server:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: cargo test
      - run: docker build -t wm-server -f docker/Dockerfile.server .
      - run: docker push registry/wm-server:latest
      # Blue-green deploy com drain de conexões
      - name: Deploy (zero-downtime)
        run: |
          ssh vps << 'EOF'
            # 1. Pull nova imagem
            docker compose -f docker/compose.yml pull

            # 2. Sobe novo container em paralelo (blue)
            docker compose -f docker/compose.yml up -d --no-deps --scale wm-server=2 wm-server

            # 3. Aguarda health check do novo container (max 30s)
            NEW_CONTAINER=$(docker compose ps -q wm-server | tail -n1)
            timeout 30 bash -c "until docker inspect --format='{{.State.Health.Status}}' $NEW_CONTAINER 2>/dev/null | grep -q healthy; do sleep 2; done"

            # 4. Drena conexões do container antigo via HAProxy
            OLD_CONTAINER=$(docker compose ps -q wm-server | head -n1)
            docker exec haproxy sh -c "echo 'set server wm-backend/old state drain' | socat stdio /var/run/haproxy/admin.sock"

            # 5. Aguarda drain (jogadores migram no próximo tick de reconexão)
            sleep 15

            # 6. Remove container antigo
            docker stop $OLD_CONTAINER && docker rm $OLD_CONTAINER
            docker compose -f docker/compose.yml up -d --no-deps --scale wm-server=1 wm-server
          EOF
```

### Backup Automático do PostgreSQL

```bash
# Cron diário no VPS do banco
0 4 * * * pg_dump weapons_masters | gzip > /backups/wm_$(date +%Y%m%d).sql.gz
# Manter últimos 30 dias
find /backups -name "wm_*.sql.gz" -mtime +30 -delete
```

---

## Arte e Polish

- [ ] Modelos low-poly estilizados: jogador (3 classes), 5 tipos de mob, 2 bosses
- [ ] Mapas: Plains (aberto, árvores) + Forest (denso, escuro) + Dungeon (caverna)
- [ ] UI final: login, HUD, inventário, chat, trade, party frame, minimap
- [ ] Efeitos de skills: particles (Godot GPUParticles3D) + shaders simples
- [ ] Música: 1 track por mapa (royalty-free ou AI-generated)
- [ ] SFX: hit, dodge, skill cast, level up, loot drop

---

## Checklist

- [ ] 3 classes com 4 skills cada (12 SkillDefs no servidor)
- [ ] Seleção de classe na criação de personagem
- [ ] Dungeon instanciada (World Server temporário)
- [ ] Boss com mecânica de AoE telegrafado + enrage
- [ ] Party system (convite, HP grupo, loot sharing)
- [ ] Gold sinks: reparo, taxa AH, enhance, teleporte, poções
- [ ] VPS contratado e configurado (Hetzner)
- [ ] Docker Compose de produção (com env vars de prod)
- [ ] CDN configurado para assets Web (Cloudflare R2)
- [ ] CI/CD: push para main → build → test → deploy blue-green (zero-downtime)
- [ ] Backup automático diário do PostgreSQL
- [ ] Modelos low-poly para classes, mobs, bosses
- [ ] 2 mapas + 1 dungeon com arte básica
- [ ] UI polida (login → HUD → inventário → chat → trade → party)
- [ ] Efeitos visuais de skills (particles)
- [ ] Landing page simples (HTML) com link para jogar no browser
- [ ] Formulário de inscrição para alpha testers
- [ ] **Load test**: 50 bots por 4 horas contínuas
- [ ] **Stress test real**: convidar 20-50 pessoas para fim de semana de teste
- [ ] **Commit: "Step 5 done — alpha ready"**

## Critério de Pronto

100 jogadores simultâneos jogando por um fim de semana. A economia não quebrou. Nenhum exploit de duplicação. Servidor estável. Players completaram uma dungeon em grupo. Funciona no Chrome, PC nativo e Android.

## Armadilhas deste Step

| Armadilha | Solução |
|:---|:---|
| `docker compose up -d` derruba conexões ativas durante deploy | Blue-green deploy: sobe container novo → health check → drain do antigo via HAProxy |
| Health check falha e novo container não sobe | Timeout de 30s + rollback automático (mantém container antigo ativo) |
| Drain muito curto perde jogadores no meio de dungeon | Aguardar 15s de drain + notificar jogadores com overlay "Servidor atualizando..." |

---

## Depois do Alpha

O que vem após o Step 5 (não faz parte deste roadmap, mas para referência):

- **Beta**: Mais classes, PvP arenas ranked, sistema de guildas, housing
- **Produção**: Kubernetes + Agones, escala elástica, múltiplas regiões
- **Monetização**: Battle pass, skins cosméticas (nunca pay-to-win)
- **Console**: Porte para PS5/Xbox/Switch via parceiros terceirizados
