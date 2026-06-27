use bevy_ecs::prelude::*;
use std::sync::Arc;
use std::time::Instant;
use shared::proto::{EntityAction, EntityState, Vec2, WorldSnapshot};
use crate::components::{CombatState, Health, NetworkIdentity, Position, PositionHistory};
use crate::resources::{CombatEventQueue, GlobalState, SnapshotCache, SnapshotSender};
use crate::mobs;
use crate::experience;

pub fn record_position_history_system(mut query: Query<(&Position, &mut PositionHistory)>) {
    let now = Instant::now();
    for (position, mut history) in query.iter_mut() {
        history.push(now, *position);
    }
}

pub fn build_and_broadcast_snapshot_system(
    global_state: Res<GlobalState>,
    mut events: ResMut<CombatEventQueue>,
    snapshot_sender: Res<SnapshotSender>,
    mut cache: ResMut<SnapshotCache>,
    query: Query<(&NetworkIdentity, &Position, &Health, &CombatState)>,
    mob_query: Query<(&mobs::MobIdentity, &mobs::MobPosition, &mobs::MobHealth, &mobs::MobState)>,
    mut levelup_queue: ResMut<experience::LevelUpEventQueue>,
    mut rewards: ResMut<mobs::RewardEventQueue>,
) {
    cache.entities.clear();
    for (identity, position, health, combat_state) in query.iter() {
        cache.entities.push(EntityState {
            entity_id: identity.entity_id,
            position: Some(Vec2 { x: position.x, y: position.y }),
            rotation: combat_state.rotation,
            collision_flags: combat_state.collision_flags,
            last_processed_input: combat_state.last_processed_input,
            hp: health.current,
            max_hp: health.max,
            current_action: combat_state.action as i32,
        });
    }
    cache.entities.sort_by_key(|entity| entity.entity_id);

    cache.mob_entities.clear();
    for (identity, position, health, state) in mob_query.iter() {
        let action = match state {
            mobs::MobState::Idle => EntityAction::Idle as i32,
            mobs::MobState::Aggro { .. } => EntityAction::Moving as i32,
            mobs::MobState::Attack { .. } => EntityAction::Casting as i32,
            mobs::MobState::Dead { .. } => EntityAction::Dead as i32,
        };
        cache.mob_entities.push(EntityState {
            entity_id: identity.mob_id,
            position: Some(Vec2 { x: position.x, y: position.y }),
            rotation: 0.0,
            collision_flags: 0,
            last_processed_input: 0,
            hp: health.current,
            max_hp: health.max,
            current_action: action,
        });
    }

    use shared::proto::LevelUpEvent as ProtoLevelUp;
    let level_up_events: Vec<ProtoLevelUp> = levelup_queue.events.drain(..)
        .map(|e| ProtoLevelUp {
            entity_id: e.entity_id,
            new_level: e.new_level as i32,
            new_experience: e.new_experience as i64,
        })
        .collect();

    use shared::proto::{ItemData, LootDrop};
    let mut loot_drops: Vec<LootDrop> = Vec::new();
    for loot_event in rewards.loot_events.drain(..) {
        for (slot_offset, item) in loot_event.items.iter().enumerate() {
            loot_drops.push(LootDrop {
                entity_id: loot_event.entity_id,
                slot: slot_offset as i32,
                item: Some(ItemData {
                    item_id: item.item_id,
                    item_name: item.item_name.to_string(),
                    quantity: 1,
                }),
            });
        }
    }

    let snapshot = WorldSnapshot {
        tick: global_state.tick,
        last_processed_input: global_state.last_processed_input,
        entities: std::mem::replace(&mut cache.entities, Vec::with_capacity(256)),
        combat_events: std::mem::take(&mut events.events),
        local_entity_id: 0,
        level_up_events,
        loot_drops,
        mob_entities: std::mem::replace(&mut cache.mob_entities, Vec::with_capacity(64)),
        session_reauth_challenge: None,
        session_reauth_result: None,
    };

    use prost::Message;
    let mut payload = Vec::with_capacity(snapshot.encoded_len());
    if snapshot.encode(&mut payload).is_ok() {
        if snapshot_sender.0.send(Arc::new(payload)).is_err() {
            tracing::debug!("No connected clients — snapshot dropped");
        }
    }
}
