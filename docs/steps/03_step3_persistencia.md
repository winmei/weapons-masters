# Step 3 — Mundo Persistente com Mobs e Progressão (4-5 semanas)

## Objetivo

Um mapa com mobs que dão XP. O jogador ganha nível, equipa itens e seu progresso é salvo no PostgreSQL. Ao deslogar e relogar, tudo está lá. **Arte básica (low-poly ou assets gratuitos).**

## Pré-requisito: Step 2 completo (combate 1v1 com dodge funcionando)

---

## Infraestrutura Nova (Docker Compose)

```yaml
# docker/compose.yml
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: weapons_masters
      POSTGRES_USER: wm
      POSTGRES_PASSWORD: wm_dev
    ports: ["5432:5432"]
    volumes: ["pg_data:/var/lib/postgresql/data"]

  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]

  nats:
    image: nats:2-alpine
    command: ["--jetstream", "--store_dir=/data"]
    ports: ["4222:4222", "8222:8222"]
    volumes: ["nats_data:/data"]

volumes:
  pg_data:
  nats_data:
```

---

## Schema do Banco

```sql
-- migrations/001_initial.sql
CREATE TABLE players (
    id          BIGSERIAL PRIMARY KEY,
    username    VARCHAR(32) UNIQUE NOT NULL,
    password_hash VARCHAR(128) NOT NULL,
    created_at  TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE player_characters (
    id          BIGSERIAL PRIMARY KEY,
    player_id   BIGINT REFERENCES players(id),
    name        VARCHAR(32) NOT NULL,
    level       INT DEFAULT 1,
    experience  BIGINT DEFAULT 0,
    hp          INT DEFAULT 100,
    max_hp      INT DEFAULT 100,
    position_x  REAL DEFAULT 0.0,
    position_y  REAL DEFAULT 0.0,
    position_map VARCHAR(32) DEFAULT 'starter',
    stats       JSONB DEFAULT '{"str":10,"dex":10,"int":10}'::jsonb,
    updated_at  TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE player_inventory (
    character_id BIGINT REFERENCES player_characters(id),
    slot         SMALLINT,
    item_data    JSONB NOT NULL,
    PRIMARY KEY (character_id, slot)
);
```

---

## Novos Crates Rust

### `persistence` (DB Sync Worker)

```rust
use async_nats::Client;
use sqlx::PgPool;
use futures_util::StreamExt;

// Consome eventos da fila NATS e grava no PostgreSQL de forma concorrente
pub async fn run_db_sync(nats: Client, pool: PgPool) {
    let mut consumer = nats.subscribe("persistence.>")
        .await
        .expect("Falha explícita ao assinar a fila de persistência NATS");
    
    while let Some(msg) = consumer.next().await {
        let pool_clone = pool.clone(); // O PgPool internamente partilha estado via Arc
        
        // Spawn isolado para cada mensagem: evita que I/O lento
        // bloqueie a ingestão de outros pacotes da fila NATS.
        tokio::spawn(async move {
            match msg.subject.as_str() {
                "persistence.snapshot" => {
                    handle_snapshot(&pool_clone, &msg.payload).await;
                }
                "persistence.event.levelup" => {
                    handle_levelup(&pool_clone, &msg.payload).await;
                }
                "persistence.event.loot" => {
                    handle_loot(&pool_clone, &msg.payload).await;
                }
                unknown_topic => {
                    // Sem falhas silenciosas: tópicos desconhecidos
                    // devem ser registados para triagem arquitetural.
                    tracing::error!(
                        topic = unknown_topic,
                        payload_size = msg.payload.len(),
                        "Tópico não reconhecido no DB Sync Worker. Ação ignorada. Possível descompasso arquitetural de publicação."
                    );
                }
            }
        });
    }
}

async fn handle_snapshot(pool: &PgPool, payload: &[u8]) { /* Processamento ACID */ }
async fn handle_levelup(pool: &PgPool, payload: &[u8]) { /* Processamento ACID */ }
async fn handle_loot(pool: &PgPool, payload: &[u8]) { /* Processamento ACID */ }
```

### `services/auth`

```rust
use sqlx::PgPool;
use std::net::IpAddr;
use std::time::Duration;
use tokio::task;

pub struct SecurityConfig {
    pub jwt_secret: String,
}

pub fn create_secure_jwt(player_id: i64, expiration: Duration, config: &SecurityConfig) -> String {
    let exp_timestamp = expiration.as_secs();
    let header = "{\"alg\":\"HS256\",\"typ\":\"JWT\"}";
    let payload = format!("{{\"sub\":\"{}\",\"exp\":{}}}", player_id, exp_timestamp);
    
    // Simulação do encode base64 url-safe e HMAC-SHA256 (delegar para crates robustas como jsonwebtoken)
    let encoded_data = format!("{}.{}", header, payload);
    let signature = generate_hmac_sha256(&encoded_data, &config.jwt_secret);
    
    format!("{}.{}", encoded_data, signature)
}

fn generate_hmac_sha256(data: &str, secret: &str) -> String {
    // Mock representativo para a geração criptográfica
    format!("signature_hash_com_secret_{}", secret.len())
}

/// Login: verifica password (em thread bloqueante) → retorna JWT + vincula IP.
///
/// O `verify_password` (Argon2) é intencionalmente lento (~50-200ms de CPU).
/// Executá-lo diretamente na thread async do Tokio paralisaria o event loop,
/// causando picos de latência em todos os outros jogadores conectados ao Gateway.
/// A solução é delegar para `task::spawn_blocking`.
pub async fn login(
    pool: &PgPool,
    redis: &mut RedisConn,
    config: &SecurityConfig,
    username: String,     // owned — será movido para a closure blocking
    password: String,     // owned — idem
    origin_ip: IpAddr,
) -> Result<String, AuthError> {
    let player = sqlx::query!(
        "SELECT id, password_hash FROM players WHERE username = $1",
        username
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(?error, "Falha na base de dados ao procurar utilizador");
        AuthError::InternalError
    })?
    .ok_or(AuthError::NotFound)?;

    // Delega Argon2/Bcrypt para thread pool bloqueante.
    // Impede starvation do event loop do Tokio.
    let hash_to_verify = player.password_hash;
    let is_valid = task::spawn_blocking(move || {
        verify_password(&password, &hash_to_verify)
    })
    .await
    .map_err(|error| {
        tracing::error!(?error, "Falha catastrófica na thread pool de blocking");
        AuthError::InternalError
    })??;

    if !is_valid {
        return Err(AuthError::InvalidPassword);
    }

    let token = create_secure_jwt(player.id, Duration::from_secs(900), config); // 15 min

    // Vincula o IP de origem à sessão no Redis.
    // O Gateway validará este IP em cada pacote recebido.
    let session_key = format!("session:{}:ip", player.id);
    redis.set_ex(&session_key, origin_ip.to_string(), 900)
        .await
        .map_err(|error| {
            tracing::error!(?error, "Falha ao gravar IP da sessão no Redis");
            AuthError::InternalError
        })?;

    Ok(token)
}
```

```rust
// Gateway: valida IP da conexão contra o IP vinculado à sessão.
// Estratégia diferenciada por plataforma para evitar desconexões
// injustificadas em redes móveis (handoff 4G → WiFi).
async fn validate_session_ip(
    redis: &mut RedisConn,
    player_id: i64,
    connection_ip: IpAddr,
    client_type: ClientType,  // PC, Mobile, Web
) -> Result<SessionAction, SessionError> {
    let session_key = format!("session:{}:ip", player_id);
    let stored_ip: Option<String> = redis.get(&session_key).await?;
    
    match stored_ip {
        None => Err(SessionError::SessionRevoked),
        Some(ip) if ip == connection_ip.to_string() => {
            Ok(SessionAction::Allow) // IP confere, sessão válida
        }
        Some(ip) => {
            // IP divergente — ação depende da plataforma.
            tracing::warn!(
                player_id, %connection_ip, stored = %ip, ?client_type,
                "IP da conexão diverge da sessão original"
            );
            
            match client_type {
                ClientType::PC => {
                    // PC: rejeição imediata — troca de IP em desktop
                    // é forte indicador de roubo de sessão.
                    Err(SessionError::IpMismatch)
                }
                ClientType::Mobile | ClientType::Web => {
                    // Mobile/Web: troca de IP é comum (handoff de rede).
                    // Em vez de desconectar, emite re-auth challenge.
                    // O cliente tem 10s para revalidar via refresh token.
                    Ok(SessionAction::ReAuthChallenge {
                        deadline: Instant::now() + Duration::from_secs(10),
                        new_ip: connection_ip,
                    })
                }
            }
        }
    }
}

enum SessionAction {
    Allow,
    ReAuthChallenge { deadline: Instant, new_ip: IpAddr },
}

#[derive(Debug)]
enum ClientType { PC, Mobile, Web }
```

---

## IA de Mobs (Server-side)

```rust
enum MobState { Idle, Patrol(Vec2), Aggro(Entity), Attack(Entity), Dead(Instant) }

/// Sistema de IA dos mobs com validação estrita de entidades.
///
/// Se o jogador alvo fechar o jogo, efetuar logoff ou for transferido
/// de mapa, a entidade deixará de existir no ECS. Sem validação,
/// `world.get::<Position>(target)` causaria panic, derrubando o
/// World Server inteiro. A solução é verificar a existência do alvo
/// e reverter graciosamente para Idle se ele desaparecer.
fn mob_ai_system(world: &mut World, spatial: &SpatialHash, dt: f32) {
    for (mob_entity, mob_pos, mob_state, mob_def) in world.query_mob() {
        match mob_state {
            MobState::Idle => {
                // Procura jogador próximo (aggro range = 10 unidades)
                if let Some(player) = spatial.nearest_player(mob_pos, 10.0) {
                    *mob_state = MobState::Aggro(player);
                }
            }
            MobState::Aggro(target) => {
                // Valida que o alvo ainda existe no ECS antes de acessar.
                if let Some(target_pos) = world.get::<Position>(target) {
                    move_toward(mob_pos, target_pos, mob_def.speed, dt);
                    if mob_pos.distance(target_pos) <= mob_def.attack_range {
                        *mob_state = MobState::Attack(target);
                    }
                } else {
                    // Alvo desapareceu (logoff, mapa, morte) → volta a Idle.
                    tracing::debug!(
                        ?mob_entity, ?target,
                        "Alvo do mob não existe mais no ECS — revertendo para Idle"
                    );
                    *mob_state = MobState::Idle;
                }
            }
            MobState::Attack(target) => {
                // Mesma validação — alvo pode morrer/desconectar entre ticks.
                if world.get::<Position>(target).is_some() {
                    apply_mob_damage(world, mob_entity, target);
                } else {
                    *mob_state = MobState::Idle;
                }
            }
            MobState::Dead(time) => {
                // Respawn após 30 segundos
                if time.elapsed() > Duration::from_secs(30) {
                    respawn_mob(world, mob_entity);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod mob_tests {
    use super::*;

    #[test]
    fn mob_deve_reverter_para_idle_quando_alvo_desaparecer() {
        // Arrange: mob em Aggro com target entity 99
        let mut world = World::new();
        let mob = world.spawn((
            Position(Vec2::ZERO),
            MobState::Aggro(Entity::from_raw(99)),
            MobDefinition { speed: 5.0, attack_range: 3.0 },
        ));
        // Entity 99 NÃO existe no world → simula logoff do jogador.

        // Act
        mob_ai_system(&mut world, &SpatialHash::new(20.0), 0.033);

        // Assert: mob voltou a Idle
        let state = world.get::<MobState>(mob).unwrap();
        assert!(matches!(state, MobState::Idle));
    }
}
```

---

## Fluxo de Persistência

```
1. Jogador mata mob → XP event → NATS "persistence.event.levelup" (imediato)
2. Jogador pega loot → Item event → NATS "persistence.event.loot" (imediato)
3. A cada 30s → Snapshot de todos os players → NATS "persistence.snapshot" (batch)
4. Ctrl+C no server → Graceful shutdown → flush forçado de todos → NATS → PG
5. DB Sync Worker consome fila → UPSERT no PostgreSQL → ACK
```

---

## Checklist

- [x] `docker/compose.yml` com PostgreSQL + Redis + NATS
- [x] `docker/compose.yml` monta `./migrations:/docker-entrypoint-initdb.d` (schema aplicado automaticamente no primeiro `docker compose up`)
- [x] Schema SQL criado (`docker/migrations/001_initial.sql`)
- [x] Crate `persistence`: DB Sync Worker (NATS → PostgreSQL) com `run_db_sync`
- [x] Crate `services/auth`: registro + login + JWT (sqlx + argon2 + jsonwebtoken)
- [x] Auth: `verify_password` via `task::spawn_blocking` (não bloquear Tokio)
- [x] Refresh token rotativo no Redis (`auth` crate: one-time use + detecção de reuse)
- [x] IP binding: login vincula IP da sessão no Redis
- [x] Gateway: `validate_session_ip()` em cada pacote recebido (`gateway/src/session.rs` + worker Redis em background)
- [x] Gateway: `GameAuthPacket` no WebTransport (4433/UDP) + `ClientPlatform` no handshake
- [x] Gateway: notificação proativa `SessionReAuthChallenge` no snapshot (Mobile/Web handoff)
- [x] Gateway: porta 8081 separada para auth (LoginRequest/RegisterRequest não chegam ao canal de input do jogo)
- [x] Carregar CharacterData do PG ao logar → enviar ao cliente (auth_service.rs: handle_login + load_character_data)
- [x] Salvar estado via NATS JetStream a cada 30s (emit_persistence_events_system: SNAPSHOT_INTERVAL_TICKS=900 @ 30Hz)
- [x] Graceful shutdown: SIGTERM → flush → NATS → PG (await_shutdown_signal + flush_all_players_on_shutdown)
- [x] `MobDefinition`: tipo, HP, dano, loot table, respawn time
- [x] 3 tipos de mob: Goblin (fraco), Orc (médio), Troll (forte)
- [x] `MobAI` state machine (Idle → Aggro → Attack → Dead → Respawn)
- [x] `ExperienceSystem`: XP por mob → level up (threshold N×100)
- [x] `LootSystem`: drop table aleatório com pseudo-RNG determinístico
- [x] **`process_player_vs_mob_system`**: jogadores podem atacar mobs com skills (Tab + 1/2)
- [x] Mobs incluídos no `WorldSnapshot` via campo `mob_entities` (field 8)
- [x] `LevelUpEvent` e `LootDrop` incluídos no snapshot (fields 6 e 7)
- [x] Proto: `LoginRequest`, `LoginResponse`, `CharacterData`, `LevelUpEvent`, `LootDrop`
- [x] Client: tela de login (username + password + botão Login/Register)
- [x] Client: renderização de mobs (cubos vermelhos) com HP bar
- [x] Client: Tab seleciona mobs e jogadores (lista unificada)
- [x] Client: floating text para level up (dourado) e loot (verde)
- [x] Client: carregar mundo após login com dados do personagem (PacketHandler.InitializeHudFromSession + GameAuthPacket handshake)
- [x] Client: painel de inventário (tecla "I") (InventoryPanel.cs + open_inventory action keycode 73)
- [x] Client: barra de XP + indicador de nível (XpBar + XpLabel + CharacterInfoLabel em Main.tscn)
- [x] Client: mapa básico (terreno com árvores, low-poly ou assets gratuitos) (MapSetup.cs: 4 paredes + 8 árvores procedurais)
- [ ] **Teste**: criar conta → logar → matar mobs → nível up → pegar loot → deslogar → relogar → tudo salvo
- [ ] **Teste**: matar server (Ctrl+C) → reiniciar → progresso mantido
- [ ] **Commit: "Step 3 done — persistent world with mobs, XP, inventory"**

## Critério de Pronto

Criar conta, logar, matar 10 mobs, subir de nível, pegar 3 itens de loot. Fechar o jogo. Reabrir. Logar novamente. Nível, XP, inventário e posição no mapa estão exatamente como antes.

## Armadilhas deste Step

| Armadilha | Solução |
|:---|:---|
| `sqlx` compile-time queries exigem DB rodando durante `cargo build` | Use `sqlx::query!` com `SQLX_OFFLINE=true` + arquivo `.sqlx/` commitado |
| NATS JetStream precisa de stream criado antes de publicar | Crie streams no startup do server: `js.get_or_create_stream("persistence")` |
| Mobs de todos os jogadores são processados no mesmo tick | Use spatial partitioning: só processe mobs em células com jogadores próximos |
| JWT roubado permite injeção de comandos até expirar | Vincule IP de origem à sessão no Redis; Gateway valida IP a cada pacote |
| `verify_password` (Argon2) bloqueia event loop do Tokio por ~200ms | Delegar para `task::spawn_blocking`; nunca executar CPU-bound na thread async |
