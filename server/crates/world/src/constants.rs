use std::time::Duration;

pub const TICK_RATE: u64 = 30;
pub const TICK_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TICK_RATE);
pub const TICK_DELTA: f32 = 1.0 / TICK_RATE as f32;
pub const MAX_NET_READ_BUDGET: Duration = Duration::from_millis(8);
pub const PLAYER_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(5);
pub const PLAYER_DISCONNECT_GRACE: Duration = Duration::from_secs(30);
pub const PLAYER_SPEED_UNITS_PER_SECOND: f32 = 5.0;
pub const DODGE_DISTANCE: f32 = 3.0;
pub const DODGE_IFRAMES: Duration = Duration::from_millis(300);
pub const DODGE_COOLDOWN: Duration = Duration::from_millis(1500);
pub const HISTORY_LEN: usize = 12;
pub const PLAYER_HALF_EXTENT: f32 = 0.5;
pub const ARENA_LIMIT: f32 = 8.0;
pub const WALL_MIN_X: f32 = -2.5;
pub const WALL_MAX_X: f32 = 2.5;
pub const WALL_MIN_Y: f32 = 2.5;
pub const WALL_MAX_Y: f32 = 3.0;
pub const GRID_WIDTH: usize = 64;
pub const GRID_HEIGHT: usize = 64;
pub const CELL_COUNT: usize = GRID_WIDTH * GRID_HEIGHT;
pub const SPATIAL_NONE: u32 = u32::MAX;
pub const DEFAULT_SPATIAL_CAPACITY: usize = 2048;

pub const COL_BOTTOM: u32 = 0b0001;
pub const COL_TOP: u32    = 0b0010;
pub const COL_LEFT: u32   = 0b0100;
pub const COL_RIGHT: u32  = 0b1000;

pub const SNAPSHOT_INTERVAL_TICKS: u32 = 900;
