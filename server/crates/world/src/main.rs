mod mobs;
mod experience;
mod publisher;
mod auth_service;

use bevy_ecs::prelude::*;
use shared::proto::PlayerInput;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

pub mod constants;
pub use constants::*;

pub mod components;
pub use components::*;

pub mod spatial;
pub use spatial::*;

pub mod resources;
pub use resources::*;

pub mod combat;
pub use combat::*;

pub mod systems;
pub use systems::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    tracing::info!("World server started. Target tick rate: {}Hz", TICK_RATE);

    // ---------------------------------------------------------------------------
    // Auth service — init PostgreSQL + Redis + SecurityConfig from env.
    // Fails fast at startup if JWT_SECRET is missing or weak (per $wm-persistence-auth).
    // ---------------------------------------------------------------------------

    // Channel for auth task → ECS world: after login, the auth handler pushes
    // the real character_id so the tick loop can set it on the player entity.
    let (enter_world_tx, enter_world_rx) =
        mpsc::channel::<EnterWorldCommand>(256);

    let auth_state = match auth_service::init_auth_state(enter_world_tx).await {
        Ok(s) => {
            tracing::info!("Auth service ready");
            Some(s)
        }
        Err(e) => {
            tracing::warn!("Auth service unavailable — login/register disabled: {e}");
            None
        }
    };

    // ---------------------------------------------------------------------------
    // Persistence publisher — NATS channel between tick loop and JetStream.
    // The hot tick uses try_send (non-blocking). The publisher task awaits acks.
    // JoinHandle saved so we can await drain on graceful shutdown.
    // ---------------------------------------------------------------------------
    let nats_url = std::env::var("NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let (persistence_tx, persistence_rx) = publisher::persistence_channel();

    let publisher_handle = tokio::spawn(publisher::run_persistence_publisher(
        nats_url.clone(),
        persistence_rx,
    ));
    tracing::info!(%nats_url, "Persistence publisher task spawned");

    // ---------------------------------------------------------------------------
    // Auth gateway on port 8081 — only started when auth state is available.
    // ---------------------------------------------------------------------------
    if let Some(ref state) = auth_state {
        let handlers = auth_service::build_auth_handlers(std::sync::Arc::clone(state));
        tokio::spawn(gateway::run_auth_gateway(handlers));
        tracing::info!("Auth gateway spawned on TCP {}", gateway::DEFAULT_AUTH_PORT);
    }

    // ---------------------------------------------------------------------------
    // Session IP validator — background Redis worker (never blocks packet loop).
    // ---------------------------------------------------------------------------
    let (_session_validator, session_validate_tx) = if auth_state.is_some() {
        let redis_url = std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        match gateway::SessionValidator::spawn(&redis_url).await {
            Ok((validator, tx)) => {
                tracing::info!("Session IP validator ready");
                (Some(validator), Some(tx))
            }
            Err(error) => {
                return Err(format!(
                    "auth enabled but SessionValidator failed to connect to Redis: {error}"
                )
                .into());
            }
        }
    } else {
        (None, None)
    };

    // ---------------------------------------------------------------------------
    // Game networking channels
    // ---------------------------------------------------------------------------
    let (input_tx, input_rx) = mpsc::channel::<PlayerInput>(4096);
    let (snapshot_tx, _) = broadcast::channel::<Arc<Vec<u8>>>(128);

    let mut gateway_handle = tokio::spawn({
        let snapshot_tx = snapshot_tx.clone();
        // Build the game-auth handler only when auth state is available.
        // When None, GameAuthPacket is silently accepted but ignored (anonymous / dev mode).
        let game_auth = auth_state.as_ref().map(|state| {
            auth_service::build_game_auth_handler(Arc::clone(state))
        });
        let session_reauth = auth_state.as_ref().map(|state| {
            auth_service::build_session_reauth_handler(Arc::clone(state))
        });
        async move {
            let config = gateway::GatewayConfig {
                game_auth,
                session_reauth,
                session_validate_tx,
            };
            if let Err(error) = gateway::run_gateway_with_config(input_tx, snapshot_tx, config).await {
                tracing::error!("Gateway stopped: {}", error);
            }
        }
    });

    // ---------------------------------------------------------------------------
    // Shutdown flag — shared between async main and the game OS thread.
    // AtomicBool avoids Mutex overhead in the hot tick check path.
    // ---------------------------------------------------------------------------
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let game_shutdown_flag = Arc::clone(&shutdown_flag);

    // ---------------------------------------------------------------------------
    // Game loop — dedicated OS thread; owns the ECS world exclusively.
    // PersistenceSender is moved in and stored as a Resource so ECS systems
    // can call try_emit() without any async or mutex involvement.
    // ---------------------------------------------------------------------------
    let game_loop_handle = std::thread::spawn(move || {
        let mut world = World::new();
        world.insert_resource(InputReceiver(input_rx));
        world.insert_resource(SnapshotSender(snapshot_tx));
        world.insert_resource(GlobalState::default());
        world.insert_resource(EntityIndex::default());
        world.insert_resource(CharacterEntityIndex::default());
        world.insert_resource(SpatialHash::default());
        world.insert_resource(CombatEventQueue::default());
        world.insert_resource(SnapshotCache::default());
        world.insert_resource(NetworkInputBuffer::default());
        world.insert_resource(mobs::MobIndex::default());
        world.insert_resource(mobs::RewardEventQueue::default());
        world.insert_resource(mobs::PlayerPositionsSnapshot::default());
        world.insert_resource(experience::LevelUpEventQueue::default());
        world.insert_resource(CombatBuffer::default());
        // PersistenceSender: try_send is non-blocking — safe in the hot tick.
        world.insert_resource(PersistenceSenderResource(persistence_tx));
        // EnterWorldReceiver: authenticated spawn/restore commands for the ECS.
        world.insert_resource(EnterWorldReceiver(enter_world_rx));
        mobs::spawn_starter_mobs(&mut world);

        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                enter_world_system,
                process_network_inputs_system,
                cleanup_disconnected_system,
                cleanup_disconnected_timeout_system,
                apply_movement_and_dodge_system,
                rebuild_spatial_hash_system,
                update_status_effects_system,
                process_player_combat_skills_system,
                process_player_vs_mob_system,
                clear_unresolved_skills_system,
                mobs::update_player_positions_system,
                mobs::mob_ai_system,
                apply_mob_attacks_system,
                mobs::apply_mob_respawn_system,
                record_position_history_system,
                experience::experience_system,
                emit_persistence_events_system,
                build_and_broadcast_snapshot_system,
            )
                .chain(),
        );

        loop {
            let tick_start = Instant::now();
            schedule.run(&mut world);

            // Check shutdown flag after every full tick (max delay ≈ 33ms at 30Hz).
            // Avoids I/O or locking inside the hot path; Acquire matches the Release
            // store in main after the signal is received.
            if game_shutdown_flag.load(Ordering::Acquire) {
                tracing::info!("Game loop: shutdown signal received — flushing player snapshots");
                flush_all_players_on_shutdown(&mut world);
                tracing::info!("Game loop: graceful shutdown complete");
                // world drops here → PersistenceSenderResource drops → publisher channel closes
                break;
            }

            let elapsed = tick_start.elapsed();
            if elapsed < TICK_DURATION {
                sleep_precise(TICK_DURATION - elapsed);
            } else {
                tracing::warn!("World tick overload: {:?}", elapsed);
            }

            // Log de métricas a cada 300 ticks (~10s a 30Hz) para monitoramento.
            {
                let gs = world.resource::<GlobalState>();
                let tick = gs.tick;
                if tick % 300 == 0 && tick > 0 {
                    tracing::info!(tick, elapsed_ms = elapsed.as_secs_f64() * 1000.0, "tick metrics");
                }
            }
        }
    });

    // ---------------------------------------------------------------------------
    // Wait for shutdown signal (SIGTERM / Ctrl+C) or unexpected gateway exit.
    // ---------------------------------------------------------------------------
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received shutdown signal — initiating graceful shutdown");
        }
        _ = &mut gateway_handle => {
            tracing::warn!("Gateway task ended unexpectedly — initiating shutdown");
        }
    }

    // Signal the game loop to stop after its current tick.
    shutdown_flag.store(true, Ordering::Release);

    // Wait for game loop to complete final flush and exit.
    // Blocks for at most one tick duration (≈ 33ms) — acceptable on shutdown path.
    if let Err(error) = game_loop_handle.join() {
        tracing::error!("Game loop thread panicked during shutdown: {:?}", error);
    }
    tracing::info!("Game loop exited — waiting for persistence publisher to drain NATS");

    // The game loop dropped PersistenceSenderResource → publisher channel is now closed.
    // Await the publisher task so all in-flight NATS publishes complete before exit.
    if let Err(error) = publisher_handle.await {
        tracing::error!("Persistence publisher task failed: {:?}", error);
    }

    tracing::info!("Weapons Masters server shut down cleanly");
    Ok(())
}

// Sistemas extraídos para a pasta systems/

// SpatialHash extraído para spatial.rs

// HitResult e check_hit extraídos para combat.rs

// Colisões extraídas para spatial.rs

/// Híbrido sleep+spin para precisão de timing sem consumir 100% de CPU na janela inteira.
/// O spin apenas para a diferença residual de <2ms, minimizando jitter no deadline do tick.
fn sleep_precise(remaining: Duration) {
    let deadline = Instant::now() + remaining;
    if remaining > Duration::from_millis(2) {
        std::thread::sleep(remaining - Duration::from_millis(2));
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use shared::proto::{combat_event, EntityAction, InputType, Vec2};

    fn make_enter_world_command(entity_id: u32, character_id: i64) -> EnterWorldCommand {
        EnterWorldCommand {
            entity_id,
            character_id,
            map_id: "starter".to_string(),
            level: 7,
            experience: 350,
            current_hp: 123,
            maximum_hp: 240,
            position_x: 12.5,
            position_y: -4.25,
            rotation: 1.5,
        }
    }

    fn make_world_entry_test_world() -> World {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());
        world.insert_resource(CharacterEntityIndex::default());
        world
    }

    #[test]
    fn creates_entity_when_authenticated_character_enters_world() {
        let mut world = make_world_entry_test_world();

        assert!(apply_enter_world_command(
            &mut world,
            make_enter_world_command(42, 9001),
        ));

        let entity = world.resource::<EntityIndex>().map[&42];
        assert_eq!(world.resource::<CharacterEntityIndex>().map[&9001], entity);
        assert_eq!(world.get::<CharacterId>(entity).unwrap().value, 9001);
        assert_eq!(world.get::<WorldMapId>(entity).unwrap().value, "starter");
        assert_eq!(*world.get::<Position>(entity).unwrap(), Position { x: 12.5, y: -4.25 });
        assert_eq!(world.get::<Health>(entity).unwrap().current, 123);
        assert_eq!(world.get::<Health>(entity).unwrap().max, 240);
        assert_eq!(world.get::<experience::PlayerProgress>(entity).unwrap().level, 7);
    }

    #[test]
    fn duplicate_authentication_does_not_create_two_entities() {
        let mut world = make_world_entry_test_world();
        let command = make_enter_world_command(42, 9001);

        assert!(apply_enter_world_command(&mut world, command.clone()));
        let original = world.resource::<EntityIndex>().map[&42];
        assert!(apply_enter_world_command(&mut world, command));

        assert_eq!(world.resource::<EntityIndex>().map.len(), 1);
        assert_eq!(world.resource::<CharacterEntityIndex>().map.len(), 1);
        assert_eq!(world.resource::<EntityIndex>().map[&42], original);
    }

    #[test]
    fn reconnect_reuses_entity_and_moves_authority_to_new_entity_id() {
        let mut world = make_world_entry_test_world();
        assert!(apply_enter_world_command(
            &mut world,
            make_enter_world_command(42, 9001),
        ));
        let original = world.resource::<EntityIndex>().map[&42];

        let mut reconnect = make_enter_world_command(77, 9001);
        reconnect.position_x = 31.0;
        reconnect.position_y = 9.0;
        assert!(apply_enter_world_command(&mut world, reconnect));

        assert!(!world.resource::<EntityIndex>().map.contains_key(&42));
        assert_eq!(world.resource::<EntityIndex>().map[&77], original);
        assert_eq!(world.get::<NetworkIdentity>(original).unwrap().entity_id, 77);
        assert_eq!(*world.get::<Position>(original).unwrap(), Position { x: 31.0, y: 9.0 });
    }

    #[test]
    fn incompatible_duplicate_world_entry_is_rejected() {
        let mut world = make_world_entry_test_world();
        assert!(apply_enter_world_command(
            &mut world,
            make_enter_world_command(42, 9001),
        ));

        assert!(!apply_enter_world_command(
            &mut world,
            make_enter_world_command(42, 9002),
        ));

        let entity = world.resource::<EntityIndex>().map[&42];
        assert_eq!(world.get::<CharacterId>(entity).unwrap().value, 9001);
        assert!(!world.resource::<CharacterEntityIndex>().map.contains_key(&9002));
    }

    #[test]
    fn world_entry_rejects_invalid_authoritative_identity() {
        let mut world = make_world_entry_test_world();

        assert!(!apply_enter_world_command(
            &mut world,
            make_enter_world_command(0, 9001),
        ));
        assert!(!apply_enter_world_command(
            &mut world,
            make_enter_world_command(42, 0),
        ));
        assert!(world.resource::<EntityIndex>().map.is_empty());
        assert!(world.resource::<CharacterEntityIndex>().map.is_empty());
    }

    #[test]
    fn world_entry_rejects_existing_entity_with_incomplete_components() {
        let mut world = make_world_entry_test_world();
        let entity = world.spawn((
            NetworkIdentity { entity_id: 42 },
            CharacterId { value: 9001 },
        )).id();
        world.resource_mut::<EntityIndex>().map.insert(42, entity);
        world.resource_mut::<CharacterEntityIndex>().map.insert(9001, entity);

        assert!(!apply_enter_world_command(
            &mut world,
            make_enter_world_command(42, 9001),
        ));
        assert!(world.get::<Position>(entity).is_none());
    }

    #[test]
    fn input_for_nonexistent_entity_does_not_spawn_player() {
        let mut world = World::new();
        let (input_tx, input_rx) = mpsc::channel::<PlayerInput>(4);
        input_tx.try_send(PlayerInput {
            entity_id: 404,
            sequence: 1,
            input_type: InputType::Move as i32,
            direction: Some(Vec2 { x: 1.0, y: 0.0 }),
            ..Default::default()
        }).unwrap();

        world.insert_resource(InputReceiver(input_rx));
        world.insert_resource(GlobalState::default());
        world.insert_resource(EntityIndex::default());
        world.insert_resource(NetworkInputBuffer::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(process_network_inputs_system);
        schedule.run(&mut world);

        assert!(world.resource::<EntityIndex>().map.is_empty());
        assert_eq!(world.resource::<GlobalState>().last_processed_input, 0);
        let mut identity_query = world.query::<&NetworkIdentity>();
        assert_eq!(identity_query.iter(&world).count(), 0);
    }

    fn make_spatial_hash() -> SpatialHash {
        SpatialHash::default()
    }

    #[test]
    fn check_hit_returns_hit_inside_range() {
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 1.0, y: 0.0 },
            DodgeState::new(Instant::now()),
            GOLPE,
            &make_spatial_hash(),
            Instant::now(),
        );
        assert!(matches!(result, HitResult::Hit { damage: 50 }));
    }

    #[test]
    fn check_hit_rejects_out_of_range() {
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 10.0, y: 0.0 },
            DodgeState::new(Instant::now()),
            GOLPE,
            &make_spatial_hash(),
            Instant::now(),
        );
        assert!(matches!(result, HitResult::OutOfRange));
    }

    #[test]
    fn check_hit_rejects_dodge_iframes() {
        let now = Instant::now();
        let active_dodge = DodgeState {
            iframe_until: now + Duration::from_millis(300),
            cooldown_until: now + Duration::from_millis(1500),
        };
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 1.0, y: 0.0 },
            active_dodge,
            GOLPE,
            &make_spatial_hash(),
            now,
        );
        assert!(matches!(result, HitResult::Dodged));
    }

    #[test]
    fn check_hit_rejects_blocked_line_of_sight() {
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 0.0, y: 4.0 },
            DodgeState::new(Instant::now()),
            DISPARO,
            &make_spatial_hash(),
            Instant::now(),
        );
        assert!(matches!(result, HitResult::Blocked));
    }

    #[test]
    fn spatial_hash_visits_nearby_entity() {
        let mut spatial_hash = SpatialHash::default();
        spatial_hash.clear();
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        spatial_hash.insert(Position { x: 0.0, y: 0.0 }, entity);
        let mut visited = 0;
        spatial_hash.for_nearby_entities(Position { x: 0.5, y: 0.5 }, |e| {
            if e == entity { visited += 1; }
        });
        assert_eq!(visited, 1);
    }

    #[test]
    fn apply_input_to_intent_move_sets_direction() {
        let mut intent = MovementIntent::default();
        let mut combat_state = CombatState::new(Instant::now());
        let input = PlayerInput {
            sequence: 1,
            input_type: InputType::Move as i32,
            direction: Some(Vec2 { x: 1.0, y: 0.0 }),
            ..Default::default()
        };
        apply_input_to_intent(&input, &mut intent, &mut combat_state);
        assert!(intent.direction.is_some());
    }

    #[test]
    fn apply_input_to_intent_stop_clears_direction() {
        let mut intent = MovementIntent {
            direction: Some(Vec2 { x: 1.0, y: 0.0 }),
            ..Default::default()
        };
        let mut combat_state = CombatState::new(Instant::now());
        let input = PlayerInput {
            sequence: 2,
            input_type: InputType::Stop as i32,
            ..Default::default()
        };
        apply_input_to_intent(&input, &mut intent, &mut combat_state);
        assert!(intent.direction.is_none());
    }

    #[test]
    fn position_history_sample_returns_closest_past_entry() {
        let mut history = PositionHistory::default();
        let base = Instant::now();
        history.push(base, Position { x: 1.0, y: 0.0 });
        history.push(base + Duration::from_millis(50), Position { x: 2.0, y: 0.0 });
        let sampled = history.sample_at(base + Duration::from_millis(25));
        assert!((sampled.x - 1.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Testes de reconexão e graceful disconnect
    // -----------------------------------------------------------------------

    #[test]
    fn disconnected_timeout_not_expired_within_grace() {
        let now = Instant::now();
        let disconnected = Disconnected {
            since: now,
            timeout: PLAYER_DISCONNECT_GRACE,
        };
        // Recém-desconectado: não deve expirar
        assert!(now.duration_since(disconnected.since) <= disconnected.timeout);
    }

    #[test]
    fn disconnected_timeout_expired_after_grace() {
        let now = Instant::now();
        let disconnected = Disconnected {
            since: now - Duration::from_secs(31),
            timeout: PLAYER_DISCONNECT_GRACE,
        };
        // Passou do grace: deve expirar
        assert!(now.duration_since(disconnected.since) > disconnected.timeout);
    }

    #[test]
    fn reconnection_removes_disconnected_component() {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());

        let entity = world.spawn((
            NetworkIdentity { entity_id: 42 },
            Position::default(),
            Health::default(),
            CombatState::new(Instant::now()),
            LastActive { at: Instant::now() },
            Disconnected {
                since: Instant::now(),
                timeout: PLAYER_DISCONNECT_GRACE,
            },
        )).id();

        world.resource_mut::<EntityIndex>().map.insert(42, entity);

        // Verifica que a entidade tem Disconnected
        assert!(world.get::<Disconnected>(entity).is_some());

        // Simula remoção (como faria process_network_inputs_system)
        world.entity_mut(entity).remove::<Disconnected>();

        // Verifica que Disconnected foi removido
        assert!(world.get::<Disconnected>(entity).is_none());
        // Position deve permanecer intacta
        assert!(world.get::<Position>(entity).is_some());
    }

    #[test]
    fn cleanup_system_spawns_disconnected_after_timeout() {
        let now = Instant::now();
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());

        let entity = world.spawn((
            NetworkIdentity { entity_id: 99 },
            Position::default(),
            Health::default(),
            CombatState::new(now),
            LastActive { at: now - Duration::from_secs(6) }, // > PLAYER_INACTIVITY_TIMEOUT
        )).id();

        world.resource_mut::<EntityIndex>().map.insert(99, entity);

        let mut schedule = Schedule::default();
        schedule.add_systems(cleanup_disconnected_system);
        schedule.run(&mut world);

        // Verifica que Disconnected foi inserido
        assert!(world.get::<Disconnected>(entity).is_some());
    }

    #[test]
    fn pvp_immune_component_is_not_inserted_on_disconnect() {
        let mut world = World::new();

        let entity = world.spawn((
            NetworkIdentity { entity_id: 1 },
            Position::default(),
            Health::default(),
            CombatState::new(Instant::now()),
            LastActive { at: Instant::now() - Duration::from_secs(6) },
        )).id();

        // Simula cleanup_disconnected_system — NÃO insere PvPImmune
        world.entity_mut(entity).insert(
            Disconnected { since: Instant::now(), timeout: PLAYER_DISCONNECT_GRACE },
        );

        assert!(world.get::<Disconnected>(entity).is_some());
        assert!(world.get::<PvPImmune>(entity).is_none());
    }

    #[test]
    fn segment_intersects_wall_aabb_correct() {
        // Linha que atravessa a wall (WALL_MIN_X=2.5..WALL_MAX_X=2.5, WALL_MIN_Y=2.5..WALL_MAX_Y=3.0)
        let from = Position { x: 0.0, y: 0.0 };
        let to = Position { x: 0.0, y: 5.0 };
        assert!(segment_intersects_wall(from, to));

        // Linha que não toca a wall
        let from = Position { x: -5.0, y: -5.0 };
        let to = Position { x: -3.0, y: -3.0 };
        assert!(!segment_intersects_wall(from, to));
    }

    // -----------------------------------------------------------------------
    // Testes de try_consume_skill (DRY cooldown utility)
    // -----------------------------------------------------------------------

    #[test]
    fn try_consume_skill_returns_none_when_no_pending() {
        let mut cs = CombatState::new(Instant::now());
        assert!(try_consume_skill(&mut cs, Instant::now()).is_none());
        assert!(cs.pending_skill.is_none());
    }

    #[test]
    fn try_consume_skill_returns_none_on_cooldown() {
        let now = Instant::now();
        let mut cs = CombatState::new(now);
        cs.cooldowns.insert(1, now);
        cs.pending_skill = Some((1, 100)); // GOLPE targeting entity 100
        // Cooldown não expirou (acabou de ser criado)
        assert!(try_consume_skill(&mut cs, now).is_none());
        // pending_skill foi limpo mesmo quando cooldown ativo
        assert!(cs.pending_skill.is_none());
    }

    #[test]
    fn try_consume_skill_returns_skill_after_cooldown() {
        let mut cs = CombatState::new(Instant::now() - Duration::from_secs(10));
        cs.pending_skill = Some((2, 200)); // DISPARO
        let result = try_consume_skill(&mut cs, Instant::now());
        assert!(result.is_some());
        let (skill_id, target_id, skill) = result.unwrap();
        assert_eq!(skill_id, 2);
        assert_eq!(target_id, 200);
        assert_eq!(skill.damage, 80);
        assert!(cs.pending_skill.is_none());
        // Cooldown deve ter sido atualizado
        assert!(cs.action == EntityAction::Casting);
    }

    #[test]
    fn try_consume_skill_rejects_unknown_skill_id() {
        let mut cs = CombatState::new(Instant::now() - Duration::from_secs(10));
        cs.pending_skill = Some((99, 300)); // skill_id 99 não existe
        assert!(try_consume_skill(&mut cs, Instant::now()).is_none());
        assert!(cs.pending_skill.is_none());
    }

    // -----------------------------------------------------------------------
    // Testes de DodgeResult rejection
    // -----------------------------------------------------------------------

    #[test]
    fn dodge_rejection_emits_failed_dodge_result() {
        let mut events = CombatEventQueue::default();
        let mut dodge_state = DodgeState::new(Instant::now());
        // Forçar cooldown ativo
        dodge_state.cooldown_until = Instant::now() + Duration::from_secs(5);
        let mut position = Position { x: 0.0, y: 0.0 };
        let mut combat_state = CombatState::new(Instant::now());

        apply_dodge_intent(
            42,
            Vec2 { x: 1.0, y: 0.0 },
            Instant::now(),
            &mut position,
            &mut combat_state,
            &mut dodge_state,
            &mut events,
        );

        // Position não deve ter mudado
        assert!((position.x).abs() < f32::EPSILON);
        assert!((position.y).abs() < f32::EPSILON);
        // Deve ter emitido um DodgeResult com success: false
        assert_eq!(events.events.len(), 1);
        let event = &events.events[0];
        match &event.event {
            Some(combat_event::Event::Dodge(dr)) => {
                assert_eq!(dr.entity_id, 42);
                assert!(!dr.success);
            }
            _ => panic!("Expected Dodge event with success: false"),
        }
    }

    // -----------------------------------------------------------------------
    // Testes de DroppedItem com &'static str
    // -----------------------------------------------------------------------

    #[test]
    fn dropped_item_uses_static_str() {
        let item = mobs::DroppedItem {
            item_id: 1,
            item_name: "Potion",
        };
        // &'static str é Copy + Clone
        let cloned = item.clone();
        assert_eq!(cloned.item_id, 1);
        assert_eq!(cloned.item_name, "Potion");
    }
}
