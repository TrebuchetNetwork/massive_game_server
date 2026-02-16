// massive_game_server/server/src/server/ecs_bridge.rs

use crate::core::types::{EntityId, Pickup, PlayerID, Projectile};
use crate::entities::player::ImprovedPlayerManager;
use hecs::World;
use parking_lot::RwLock;
use std::collections::HashMap;
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
    pub velocity_x: f32,
    pub velocity_y: f32,
}

#[derive(Debug, Clone)]
pub struct ProjectileComponent {
    pub id: EntityId,
    pub owner_id: PlayerID,
    pub damage: i32,
    pub velocity_x: f32,
    pub velocity_y: f32,
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EcsMode {
    Disabled,
    Mirror,
    Authoritative,
}

pub struct EcsBridge {
    world: Arc<RwLock<World>>,
    mode: EcsMode,
    rebuild_stride_frames: u64,
}

impl Default for EcsBridge {
    fn default() -> Self {
        Self::new_from_env()
    }
}

impl EcsBridge {
    pub fn new_from_env() -> Self {
        let mode = std::env::var("MGS_ECS_MODE")
            .ok()
            .map(|raw| raw.trim().to_ascii_lowercase())
            .map(|value| match value.as_str() {
                "authoritative" | "authority" | "primary" => EcsMode::Authoritative,
                "mirror" | "snapshot" | "enabled" | "on" | "true" | "1" => EcsMode::Mirror,
                _ => EcsMode::Disabled,
            })
            .unwrap_or_else(|| {
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
                if enabled {
                    EcsMode::Mirror
                } else {
                    EcsMode::Disabled
                }
            });
        let rebuild_stride_frames = std::env::var("MGS_ECS_REBUILD_STRIDE")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(1);

        Self {
            world: Arc::new(RwLock::new(World::new())),
            mode,
            rebuild_stride_frames,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.mode != EcsMode::Disabled
    }

    pub fn mode(&self) -> EcsMode {
        self.mode
    }

    pub fn is_authoritative(&self) -> bool {
        self.mode == EcsMode::Authoritative
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
        if !self.is_enabled() {
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
                    velocity_x: player_state.velocity_x,
                    velocity_y: player_state.velocity_y,
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
                    velocity_x: projectile.velocity_x,
                    velocity_y: projectile.velocity_y,
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

    pub fn apply_authoritative_reconciliation(
        &self,
        player_manager: &ImprovedPlayerManager,
        projectiles: &mut [Projectile],
        pickups: &mut [Pickup],
    ) -> EcsSnapshotStats {
        if !self.is_authoritative() {
            return EcsSnapshotStats::default();
        }

        let world = self.world.read();
        let mut player_updates = 0usize;
        let mut projectile_updates = HashMap::with_capacity(projectiles.len());
        let mut pickup_updates = HashMap::with_capacity(pickups.len());

        for (_entity, (transform, player)) in world.query::<(&Transform, &PlayerComponent)>().iter()
        {
            if let Some(mut player_state) = player_manager.get_player_state_mut(&player.id) {
                player_state.x = transform.x;
                player_state.y = transform.y;
                player_state.rotation = transform.rotation;
                player_state.health = player.health;
                player_state.alive = player.alive;
                player_updates += 1;
            }
        }

        for (_entity, (transform, projectile)) in
            world.query::<(&Transform, &ProjectileComponent)>().iter()
        {
            projectile_updates.insert(projectile.id, (transform.x, transform.y));
        }
        for projectile in projectiles.iter_mut() {
            if let Some((x, y)) = projectile_updates.get(&projectile.id).copied() {
                projectile.x = x;
                projectile.y = y;
            }
        }

        for (_entity, (transform, pickup)) in world.query::<(&Transform, &PickupComponent)>().iter()
        {
            pickup_updates.insert(pickup.id, (transform.x, transform.y, pickup.is_active));
        }
        for pickup in pickups.iter_mut() {
            if let Some((x, y, is_active)) = pickup_updates.get(&pickup.id).copied() {
                pickup.x = x;
                pickup.y = y;
                pickup.is_active = is_active;
            }
        }

        EcsSnapshotStats {
            players: player_updates,
            projectiles: projectile_updates.len(),
            pickups: pickup_updates.len(),
        }
    }
}
