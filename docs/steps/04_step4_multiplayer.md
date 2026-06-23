# Step 4 — Multiplayer Real: Social, PvP e Múltiplos Mapas (5-6 semanas)

## Objetivo

10+ jogadores jogando simultaneamente com chat, PvP, transição entre 2 mapas (seamless) e sistema de trade. Anti-cheat básico funciona. Build mobile (Android APK).

## Pré-requisito: Step 3 completo (mundo persistente com mobs, XP e inventário salvando)

---

## Funcionalidades Novas

### 1. Segundo World Server (Mapa B)

Mesmo binário Rust, configuração diferente:
```toml
# config/map_a.toml
map_id = "plains"
listen_port = 9001
mob_spawns = [...]

# config/map_b.toml
map_id = "forest"
listen_port = 9002
mob_spawns = [...]
```

O Gateway roteia o jogador para o World Server correto baseado em `position_map` do personagem.

### 2. Seamless Transition (Dual-Subscription)

```
1. Jogador no Map A se aproxima da borda (< 50 unidades)
2. Gateway pré-registra o jogador no World Server B (lê dados do Redis)
3. Gateway começa a enviar inputs para ambos os World Servers
4. Jogador cruza a linha → Gateway promove Map B como primário
5. Map A remove o jogador após 2s de timeout
6. Zero loading screen. A conexão de rede do jogador nunca cai.
```

### 3. Chat System (via NATS PubSub)

```rust
// Canais NATS:
// chat.global         → todos os jogadores online
// chat.map.{map_id}   → jogadores no mesmo mapa
// chat.whisper.{id}   → mensagem privada
// chat.party.{id}     → grupo/party

fn handle_chat(nats: &NatsClient, sender: Entity, msg: ChatMessage) {
    match msg.channel {
        ChatChannel::Global => nats.publish("chat.global", payload),
        ChatChannel::Map => nats.publish(&format!("chat.map.{}", current_map), payload),
        ChatChannel::Whisper(target) => nats.publish(&format!("chat.whisper.{}", target), payload),
    }
}
```

### 4. Trade System (ACID no Economy Service)

```rust
// Economy Service (gRPC separado) — nunca no World Server
pub async fn execute_trade(pool: &PgPool, offer: TradeOffer) -> Result<(), TradeError> {
    // 0. Barreira de domínio: impede trade consigo mesmo.
    //    Se from_character == to_character, o SELECT FOR UPDATE bloquearia
    //    a mesma linha que o DELETE+INSERT manipulam, podendo causar
    //    deadlock ou apagar o item permanentemente.
    if offer.from_character == offer.to_character {
        return Err(TradeError::InvalidTradeTarget);
    }

    let mut tx = pool.begin().await.map_err(|error| {
        tracing::error!(
            ?error,
            from_char = offer.from_character,
            to_char = offer.to_character,
            "Falha ao iniciar transação ACID de troca"
        );
        TradeError::DatabaseError
    })?;

    // 1. Bloqueia o item do remetente + lê item_data da DB (fonte verdade).
    let source_record = sqlx::query!(
        "SELECT item_data FROM player_inventory \
         WHERE character_id = $1 AND slot = $2 FOR UPDATE",
        offer.from_character, offer.from_slot
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        tracing::error!(
            ?error,
            from_char = offer.from_character,
            slot = offer.from_slot,
            "Falha ao bloquear item de origem"
        );
        TradeError::DatabaseError
    })?;

    let item_data_to_move = match source_record {
        Some(record) => record.item_data,
        None => return Err(TradeError::ItemNotFound),
    };

    // 2. Valida que o slot de destino está vazio.
    let dest_slot = sqlx::query!(
        "SELECT 1 as exists FROM player_inventory \
         WHERE character_id = $1 AND slot = $2",
        offer.to_character, offer.to_slot
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        tracing::error!(
            ?error,
            to_char = offer.to_character,
            slot = offer.to_slot,
            "Falha ao verificar slot de destino"
        );
        TradeError::DatabaseError
    })?;

    if dest_slot.is_some() {
        return Err(TradeError::SlotOccupied);
        // tx dropado → rollback automático.
    }

    // 3. Remove item do jogador A
    sqlx::query!(
        "DELETE FROM player_inventory WHERE character_id = $1 AND slot = $2",
        offer.from_character, offer.from_slot
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        tracing::error!(
            ?error,
            from_char = offer.from_character,
            slot = offer.from_slot,
            "Falha ao remover item da origem"
        );
        TradeError::DatabaseError
    })?;

    // 4. Adiciona item ao jogador B (usa item_data lido da DB, não do offer)
    sqlx::query!(
        "INSERT INTO player_inventory (character_id, slot, item_data) \
         VALUES ($1, $2, $3)",
        offer.to_character, offer.to_slot, item_data_to_move
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        tracing::error!(
            ?error,
            to_char = offer.to_character,
            slot = offer.to_slot,
            "Falha ao inserir item no destino"
        );
        TradeError::DatabaseError
    })?;

    tx.commit().await.map_err(|error| {
        tracing::error!(
            ?error,
            from_char = offer.from_character,
            to_char = offer.to_character,
            "Falha catastrófica no commit da troca"
        );
        TradeError::DatabaseError
    })?;

    Ok(())
}
```

### 5. Reconexão

```rust
fn handle_disconnect(world: &mut World, entity: Entity) {
    world.insert(entity, Disconnected { since: Instant::now(), timeout: Duration::from_secs(30) });
    world.insert(entity, PvPImmune); // Não pode ser atacado por jogadores
    // Mobs continuam atacando em PvE
}

fn handle_reconnect(world: &mut World, entity: Entity) {
    world.remove::<Disconnected>(entity);
    world.remove::<PvPImmune>(entity);
    // Envia snapshot COMPLETO (não delta) para resincronizar o cliente
}
```

### 6. Anti-Cheat (3 camadas ativas)

```rust
// Camada 1: Rate limiter no Gateway
if session.inputs_this_second > 30 { drop_input(); return; }

// Camada 2: Speed validator no World Server
let max_distance = MAX_SPEED * dt * 1.15; // 15% tolerância para lag
if new_pos.distance(old_pos) > max_distance { correct_position(entity); flag(entity); }

// Camada 3: Dodge cooldown autoritativo
if dodge_input && world.get::<DodgeState>(entity).cooldown_until > Instant::now() {
    reject_dodge(); // Ignora silenciosamente
}
```

### 7. Observabilidade (Prometheus + Grafana)

```rust
// No game loop, a cada tick:
metrics::gauge!("game.tick_rate").set(1.0 / elapsed.as_secs_f64());
metrics::gauge!("game.entities_count").set(world.entity_count() as f64);
metrics::counter!("game.inputs_processed").increment(inputs_this_tick as u64);
metrics::histogram!("game.tick_duration_ms").record(elapsed.as_millis() as f64);
```

---

## Checklist

- [ ] Segundo World Server (mesmo binário, config diferente)
- [ ] Gateway roteando para WS correto baseado no mapa
- [ ] Seamless transition: dual-subscription + handoff
- [ ] Chat via NATS PubSub (global, mapa, whisper)
- [ ] Trade system no Economy Service (transação ACID no PG)
- [ ] Trade: validação de slot vazio + `SELECT FOR UPDATE` no item de origem
- [ ] Trade: guarda contra self-trade (`from_character == to_character`)
- [ ] Reconexão: Disconnected state + PvP immune + 30s timeout + full snapshot on reconnect
- [ ] Anti-cheat: rate limiter + speed validator + dodge cooldown server-side
- [ ] Prometheus metrics (tick rate, entities, inputs, tick duration)
- [ ] Grafana dashboard (importar JSON provisioning)
- [ ] Client: chat window (TextEdit + ScrollContainer)
- [ ] Client: trade UI (2 painéis, confirmar, countdown)
- [ ] Client: overlay "Reconectando..." + auto-reconnect
- [ ] Client: transição suave entre mapas (sem loading screen)
- [ ] Client: export Android APK funcional
- [ ] Gateway Pool: 2 instâncias atrás de HAProxy ou LB simples
- [ ] **Load test**: 10 bots (scripts Rust) fazendo ações aleatórias por 1 hora
- [ ] **Commit: "Step 4 done — multiplayer with social, PvP, seamless maps"**

## Critério de Pronto

10 jogadores simultâneos (PC + browser + Android). Conversam, lutam PvP, trocam itens, andam entre 2 mapas sem loading. Servidor estável por 1 hora. Grafana mostra tick rate ≥ 28Hz.

## Armadilhas deste Step

| Armadilha | Solução |
|:---|:---|
| Dual-subscription causa updates duplicados no cliente | O Gateway marca qual WS é "primário". Cliente ignora snapshots do secundário |
| Trade race condition (2 trades simultâneos do mesmo item) | `SELECT FOR UPDATE` no início da transação PG |
| INSERT no slot de destino falha se slot ocupado → item perdido | Validar slot vazio antes do DELETE; `return Err(SlotOccupied)` com rollback automático |
| Self-trade (from == to) causa deadlock ou deleta item | Barreira de domínio: `if from == to { return Err(InvalidTradeTarget) }` antes da transação |
| Reconexão no meio de um trade | Trade auto-cancela se um dos jogadores desconectar |
| Grafana precisa de datasource configurado | Use provisioning YAML no Docker Compose para auto-configurar |
