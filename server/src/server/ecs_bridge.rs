// massive_game_server/server/src/server/ecs_bridge.rs

use crate::core::types::{EntityId, Pickup, PlayerID, Projectile};
use crate::entities::player::ImprovedPlayerManager;
use hecs::World;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerComponent {
    pub id: PlayerID,
    pub username: String,
    pub health: i32,
    pub team_id: u8,
    pub alive: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectileComponent {
    pub id: EntityId,
    pub owner_id: PlayerID,
    pub damage: i32,
}

#[derive(Debug, Clone)]
pub struct PickupComponent {
    pub id: EntityId,
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EcsSnapshotStats {
    pub players: usize,
    pub projectiles: usize,
    pub pickups: usize,
}

pub struct EcsBridge {
    world: Arc<RwLock<World>>,
    enabled: bool,
    rebuild_stride_frames: u64,
}

impl Default for EcsBridge {
    fn default() -> Self {
        Self::new_from_env()
    }
}

impl EcsBridge {
    pub fn new_from_env() -> Self {
        let enabled = std::env::var("MGS_ECS_MIGRATION_ENABLED")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                normalized == "1"
                    || normalized == "true"
                    || normalized == "yes"
                    || normalized == "on"
            })
            .unwrap_or(false);
        let rebuild_stride_frames = std::env::var("MGS_ECS_REBUILD_STRIDE")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(1);

        Self {
            world: Arc::new(RwLock::new(World::new())),
            enabled,
            rebuild_stride_frames,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn rebuild_stride_frames(&self) -> u64 {
        self.rebuild_stride_frames.max(1)
    }

    pub fn rebuild_snapshot(
        &self,
        player_manager: &ImprovedPlayerManager,
        projectiles: &[Projectile],
        pickups: &[Pickup],
    ) -> EcsSnapshotStats {
        if !self.enabled {
            return EcsSnapshotStats::default();
        }

        let mut world = self.world.write();
        *world = World::new();

        let mut player_count = 0usize;
        player_manager.for_each_player(|player_id, player_state| {
            world.spawn((
                Transform {
                    x: player_state.x,
                    y: player_state.y,
                    rotation: player_state.rotation,
                },
                PlayerComponent {
                    id: player_id.clone(),
                    username: player_state.username.clone(),
                    health: player_state.health,
                    team_id: player_state.team_id,
                    alive: player_state.alive,
                },
            ));
            player_count += 1;
        });

        for projectile in projectiles {
            let rotation = projectile.velocity_y.atan2(projectile.velocity_x);
            world.spawn((
                Transform {
                    x: projectile.x,
                    y: projectile.y,
                    rotation,
                },
                ProjectileComponent {
                    id: projectile.id,
                    owner_id: projectile.owner_id.clone(),
                    damage: projectile.damage,
                },
            ));
        }

        for pickup in pickups {
            world.spawn((
                Transform {
                    x: pickup.x,
                    y: pickup.y,
                    rotation: 0.0,
                },
                PickupComponent {
                    id: pickup.id,
                    is_active: pickup.is_active,
                },
            ));
        }

        EcsSnapshotStats {
            players: player_count,
            projectiles: projectiles.len(),
            pickups: pickups.len(),
        }
    }
}
