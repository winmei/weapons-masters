use bevy_ecs::prelude::{Entity, Resource};
use crate::components::{CombatState, Position};
use crate::constants::*;

#[derive(Default, Resource)]
pub struct SpatialHash {
    pub cell_size: f32,
    pub cells: std::collections::HashMap<usize, Vec<Entity>>,
    pub entity_cells: std::collections::HashMap<Entity, usize>,
}

impl SpatialHash {
    pub fn clear(&mut self) {
        if self.cell_size == 0.0 {
            self.cell_size = 2.0;
        }
        self.cells.clear();
        self.entity_cells.clear();
    }

    pub fn insert(&mut self, position: Position, entity: Entity) {
        self.update_entity(entity, position);
    }

    pub fn update_entity(&mut self, entity: Entity, position: Position) {
        if self.cell_size == 0.0 { self.cell_size = 2.0; }
        let cell = self.cell_index(position);
        
        if let Some(&old_cell) = self.entity_cells.get(&entity) {
            if old_cell == cell {
                return;
            }
            if let Some(list) = self.cells.get_mut(&old_cell) {
                list.retain(|&e| e != entity);
            }
        }
        
        self.entity_cells.insert(entity, cell);
        self.cells.entry(cell).or_default().push(entity);
    }

    pub fn remove_entity(&mut self, entity: Entity) {
        if let Some(old_cell) = self.entity_cells.remove(&entity) {
            if let Some(list) = self.cells.get_mut(&old_cell) {
                list.retain(|&e| e != entity);
            }
        }
    }

    pub fn cell_index(&self, position: Position) -> usize {
        let cell_x = (position.x / self.cell_size).floor() as i32;
        let cell_y = (position.y / self.cell_size).floor() as i32;
        Self::linear_index(cell_x, cell_y)
    }

    pub fn linear_index(cell_x: i32, cell_y: i32) -> usize {
        let x = cell_x.rem_euclid(GRID_WIDTH as i32) as usize;
        let y = cell_y.rem_euclid(GRID_HEIGHT as i32) as usize;
        y * GRID_WIDTH + x
    }

    pub fn is_blocked(&self, from: Position, to: Position) -> bool {
        segment_intersects_wall(from, to)
    }

    pub fn for_nearby_entities(&self, position: Position, mut visit: impl FnMut(Entity)) {
        if self.cell_size == 0.0 || self.cells.is_empty() {
            return;
        }
        let cell_x = (position.x / self.cell_size).floor() as i32;
        let cell_y = (position.y / self.cell_size).floor() as i32;
        for offset_y in -1i32..=1 {
            for offset_x in -1i32..=1 {
                let cell = Self::linear_index(cell_x + offset_x, cell_y + offset_y);
                if let Some(list) = self.cells.get(&cell) {
                    for &entity in list {
                        visit(entity);
                    }
                }
            }
        }
    }
}

pub fn resolve_world_collisions(position: &mut Position, combat_state: &mut CombatState) {
    let before_x = position.x;
    let before_y = position.y;

    position.x = position.x.clamp(
        -ARENA_LIMIT + PLAYER_HALF_EXTENT,
        ARENA_LIMIT - PLAYER_HALF_EXTENT,
    );
    position.y = position.y.clamp(
        -ARENA_LIMIT + PLAYER_HALF_EXTENT,
        ARENA_LIMIT - PLAYER_HALF_EXTENT,
    );

    if position.x != before_x {
        combat_state.collision_flags |= COL_LEFT | COL_RIGHT;
    }
    if position.y != before_y {
        combat_state.collision_flags |= COL_BOTTOM | COL_TOP;
    }

    let player_min_x = position.x - PLAYER_HALF_EXTENT;
    let player_max_x = position.x + PLAYER_HALF_EXTENT;
    let player_min_y = position.y - PLAYER_HALF_EXTENT;
    let player_max_y = position.y + PLAYER_HALF_EXTENT;

    let overlaps_wall = player_max_x > WALL_MIN_X
        && player_min_x < WALL_MAX_X
        && player_max_y > WALL_MIN_Y
        && player_min_y < WALL_MAX_Y;

    if !overlaps_wall {
        return;
    }

    let push_left  = player_max_x - WALL_MIN_X;
    let push_right = WALL_MAX_X   - player_min_x;
    let push_down  = player_max_y - WALL_MIN_Y;
    let push_up    = WALL_MAX_Y   - player_min_y;
    let min_push   = push_left.min(push_right).min(push_down).min(push_up);

    if min_push == push_left {
        position.x -= push_left;
        combat_state.collision_flags |= COL_LEFT;
    } else if min_push == push_right {
        position.x += push_right;
        combat_state.collision_flags |= COL_RIGHT;
    } else if min_push == push_down {
        position.y -= push_down;
        combat_state.collision_flags |= COL_BOTTOM;
    } else {
        position.y += push_up;
        combat_state.collision_flags |= COL_TOP;
    }
}

pub fn segment_intersects_wall(from: Position, to: Position) -> bool {
    let dx = to.x - from.x;
    let dy = to.y - from.y;

    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;

    if dx.abs() > f32::EPSILON {
        let inv_d = 1.0 / dx;
        let mut t1 = (WALL_MIN_X - from.x) * inv_d;
        let mut t2 = (WALL_MAX_X - from.x) * inv_d;
        if t1 > t2 { std::mem::swap(&mut t1, &mut t2); }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
    } else if from.x < WALL_MIN_X || from.x > WALL_MAX_X {
        return false;
    }

    if dy.abs() > f32::EPSILON {
        let inv_d = 1.0 / dy;
        let mut t1 = (WALL_MIN_Y - from.y) * inv_d;
        let mut t2 = (WALL_MAX_Y - from.y) * inv_d;
        if t1 > t2 { std::mem::swap(&mut t1, &mut t2); }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
    } else if from.y < WALL_MIN_Y || from.y > WALL_MAX_Y {
        return false;
    }

    tmin <= tmax
}
