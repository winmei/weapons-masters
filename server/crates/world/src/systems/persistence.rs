use bevy_ecs::prelude::*;
use crate::components::{CharacterId, Health, NetworkIdentity, Position, WorldMapId};
use crate::resources::{EntityIndex, GlobalState, PersistenceSenderResource};
use crate::publisher;
use crate::constants::SNAPSHOT_INTERVAL_TICKS;
use crate::experience;
use crate::mobs;

pub fn emit_persistence_events_system(
    global_state: Res<GlobalState>,
    persistence: Res<PersistenceSenderResource>,
    levelup_queue: Res<experience::LevelUpEventQueue>,
    reward_queue: Res<mobs::RewardEventQueue>,
    entity_index: Res<EntityIndex>,
    player_query: Query<(
        &NetworkIdentity,
        &Position,
        &Health,
        &experience::PlayerProgress,
        &CharacterId,
        &WorldMapId,
    )>,
) {
    use publisher::{CharacterSnapshotData, LevelUpEventData, LootDropEventData, PersistenceEvent};

    for notification in &levelup_queue.events {
        let character_id = find_character_id(&entity_index, &player_query, notification.entity_id);
        if character_id == 0 {
            if cfg!(debug_assertions) {
                tracing::debug!(
                    entity_id = notification.entity_id,
                    "emit_persistence: no character_id for level-up — skipping (dev mode or not yet loaded)"
                );
            } else {
                tracing::warn!(
                    entity_id = notification.entity_id,
                    "emit_persistence: no character_id for level-up — persistence bypass detected in production"
                );
            }
            continue;
        }
        if !persistence.0.try_emit(PersistenceEvent::LevelUp(LevelUpEventData {
            character_id,
            new_level: notification.new_level as i32,
            new_experience: notification.new_experience as i64,
        })) {
            tracing::error!(
                character_id,
                "Falha crítica ao gravar LevelUp: canal cheio ou quebrado! Podes guardar localmente numa fila de retry (WAL)."
            );
        }
    }

    for loot_event in &reward_queue.loot_events {
        let character_id = find_character_id(&entity_index, &player_query, loot_event.entity_id);
        if character_id == 0 {
            if cfg!(debug_assertions) {
                tracing::debug!(
                    entity_id = loot_event.entity_id,
                    "emit_persistence: no character_id for loot — skipping (dev mode or not yet loaded)"
                );
            } else {
                tracing::warn!(
                    entity_id = loot_event.entity_id,
                    "emit_persistence: no character_id for loot — persistence bypass detected in production"
                );
            }
            continue;
        }
        for (slot_offset, item) in loot_event.items.iter().enumerate() {
            if !persistence.0.try_emit(PersistenceEvent::LootDrop(LootDropEventData {
                character_id,
                slot: slot_offset as i16,
                item_id: item.item_id as i64,
                item_name: item.item_name.to_string(),
                quantity: 1,
            })) {
                tracing::error!(
                    character_id,
                    item_id = item.item_id,
                    "Falha crítica ao gravar LootDrop: canal cheio ou quebrado! O item não foi persistido."
                );
            }
        }
    }

    if global_state.tick % SNAPSHOT_INTERVAL_TICKS == 0 && global_state.tick > 0 {
        for (identity, position, health, progress, char_id, map_id) in player_query.iter() {
            if char_id.value == 0 {
                continue;
            }
            persistence.0.try_emit(PersistenceEvent::CharacterSnapshot(CharacterSnapshotData {
                character_id: char_id.value,
                player_id: char_id.value,
                level: progress.level as i32,
                experience: progress.experience as i64,
                hp: health.current,
                max_hp: health.max,
                position_x: position.x,
                position_y: position.y,
                position_map: map_id.value.clone(),
            }));
            let _ = identity;
        }
    }
}

pub fn find_character_id(
    entity_index: &EntityIndex,
    player_query: &Query<(
        &NetworkIdentity,
        &Position,
        &Health,
        &experience::PlayerProgress,
        &CharacterId,
        &WorldMapId,
    )>,
    entity_id: u32,
) -> i64 {
    let Some(&entity) = entity_index.map.get(&entity_id) else {
        return 0;
    };
    let Ok((_, _, _, _, char_id, _)) = player_query.get(entity) else {
        return 0;
    };
    char_id.value
}

pub fn flush_all_players_on_shutdown(world: &mut World) {
    use publisher::{CharacterSnapshotData, PersistenceEvent};

    let mut q = world.query::<(
        &Position,
        &Health,
        &experience::PlayerProgress,
        &CharacterId,
        &WorldMapId,
    )>();

    let snapshots: Vec<CharacterSnapshotData> = q
        .iter(world)
        .filter(|(_, _, _, char_id, _)| char_id.value != 0)
        .map(|(pos, health, progress, char_id, map_id)| CharacterSnapshotData {
            character_id: char_id.value,
            player_id:    char_id.value,
            level:        progress.level as i32,
            experience:   progress.experience as i64,
            hp:           health.current,
            max_hp:       health.max,
            position_x:   pos.x,
            position_y:   pos.y,
            position_map: map_id.value.clone(),
        })
        .collect();

    let count = snapshots.len();
    if count == 0 {
        tracing::info!("graceful shutdown: no authenticated players online — nothing to flush");
        return;
    }
    tracing::info!(count, "graceful shutdown: emitting final player snapshots to NATS");

    if let Some(sender) = world.get_resource::<PersistenceSenderResource>() {
        for snapshot in snapshots {
            if !sender.0.try_emit(PersistenceEvent::CharacterSnapshot(snapshot)) {
                tracing::warn!(
                    "graceful shutdown: persistence channel full — snapshot dropped; \
                     consider increasing PERSISTENCE_CHANNEL_CAPACITY"
                );
            }
        }
    } else {
        tracing::warn!("graceful shutdown: PersistenceSenderResource missing — no flush performed");
    }
}
