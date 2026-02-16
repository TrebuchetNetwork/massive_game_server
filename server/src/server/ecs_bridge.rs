// massive_game_server/server/src/server/ecs_bridge.rs

use crate::core::types::{EntityId, Pickup, PlayerID, Projectile};
use crate::entities::player::ImprovedPlayerManager;
use hecs::{Entity, World};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
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

#[derive(Default)]
struct EcsEntityIndex {
    players: HashMap<PlayerID, Entity>,
    projectiles: HashMap<EntityId, Entity>,
    pickups: HashMap<EntityId, Entity>,
}

pub struct EcsBridge {
    world: Arc<RwLock<World>>,
    entity_index: Arc<RwLock<EcsEntityIndex>>,
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
            entity_index: Arc::new(RwLock::new(EcsEntityIndex::default())),
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
        let mut entity_index = self.entity_index.write();
        let mut seen_player_ids: HashSet<PlayerID> = HashSet::new();
        let mut player_count = 0usize;
        player_manager.for_each_player(|player_id, player_state| {
            seen_player_ids.insert(player_id.clone());
            player_count += 1;

            let entity = entity_index
                .players
                .get(player_id)
                .copied()
                .filter(|entity| world.contains(*entity))
                .unwrap_or_else(|| {
                    let entity = world.spawn((
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
                    entity_index.players.insert(player_id.clone(), entity);
                    entity
                });

            if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                transform.x = player_state.x;
                transform.y = player_state.y;
                transform.rotation = player_state.rotation;
            }
            if let Ok(mut player_component) = world.get::<&mut PlayerComponent>(entity) {
                player_component.username = player_state.username.clone();
                player_component.health = player_state.health;
                player_component.team_id = player_state.team_id;
                player_component.alive = player_state.alive;
                player_component.velocity_x = player_state.velocity_x;
                player_component.velocity_y = player_state.velocity_y;
            }
        });

        let stale_players: Vec<PlayerID> = entity_index
            .players
            .keys()
            .filter(|player_id| !seen_player_ids.contains(*player_id))
            .cloned()
            .collect();
        for stale_player_id in stale_players {
            if let Some(entity) = entity_index.players.remove(&stale_player_id) {
                let _ = world.despawn(entity);
            }
        }

        let mut seen_projectile_ids: HashSet<EntityId> = HashSet::with_capacity(projectiles.len());
        for projectile in projectiles {
            seen_projectile_ids.insert(projectile.id);
            let entity = entity_index
                .projectiles
                .get(&projectile.id)
                .copied()
                .filter(|entity| world.contains(*entity))
                .unwrap_or_else(|| {
                    let rotation = projectile.velocity_y.atan2(projectile.velocity_x);
                    let entity = world.spawn((
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
                    entity_index.projectiles.insert(projectile.id, entity);
                    entity
                });

            if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                transform.x = projectile.x;
                transform.y = projectile.y;
                transform.rotation = projectile.velocity_y.atan2(projectile.velocity_x);
            }
            if let Ok(mut projectile_component) = world.get::<&mut ProjectileComponent>(entity) {
                projectile_component.owner_id = projectile.owner_id.clone();
                projectile_component.damage = projectile.damage;
                projectile_component.velocity_x = projectile.velocity_x;
                projectile_component.velocity_y = projectile.velocity_y;
            }
        }

        let stale_projectiles: Vec<EntityId> = entity_index
            .projectiles
            .keys()
            .filter(|projectile_id| !seen_projectile_ids.contains(projectile_id))
            .copied()
            .collect();
        for stale_projectile_id in stale_projectiles {
            if let Some(entity) = entity_index.projectiles.remove(&stale_projectile_id) {
                let _ = world.despawn(entity);
            }
        }

        let mut seen_pickup_ids: HashSet<EntityId> = HashSet::with_capacity(pickups.len());
        for pickup in pickups {
            seen_pickup_ids.insert(pickup.id);
            let entity = entity_index
                .pickups
                .get(&pickup.id)
                .copied()
                .filter(|entity| world.contains(*entity))
                .unwrap_or_else(|| {
                    let entity = world.spawn((
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
                    entity_index.pickups.insert(pickup.id, entity);
                    entity
                });

            if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                transform.x = pickup.x;
                transform.y = pickup.y;
                transform.rotation = 0.0;
            }
            if let Ok(mut pickup_component) = world.get::<&mut PickupComponent>(entity) {
                pickup_component.is_active = pickup.is_active;
            }
        }

        let stale_pickups: Vec<EntityId> = entity_index
            .pickups
            .keys()
            .filter(|pickup_id| !seen_pickup_ids.contains(pickup_id))
            .copied()
            .collect();
        for stale_pickup_id in stale_pickups {
            if let Some(entity) = entity_index.pickups.remove(&stale_pickup_id) {
                let _ = world.despawn(entity);
            }
        }

        if self.is_authoritative() {
            self.run_authoritative_systems(&mut world);
        }

        EcsSnapshotStats {
            players: player_count,
            projectiles: projectiles.len(),
            pickups: pickups.len(),
        }
    }

    fn run_authoritative_systems(&self, world: &mut World) {
        let fixed_dt = std::env::var("MGS_ECS_AUTHORITATIVE_FIXED_DT")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|value| *value > 0.0)
            .unwrap_or(1.0 / 60.0);

        for (_entity, (transform, player)) in world.query::<(&mut Transform, &PlayerComponent)>().iter()
        {
            transform.x += player.velocity_x * fixed_dt;
            transform.y += player.velocity_y * fixed_dt;
        }

        for (_entity, (transform, projectile)) in
            world.query::<(&mut Transform, &ProjectileComponent)>().iter()
        {
            transform.x += projectile.velocity_x * fixed_dt;
            transform.y += projectile.velocity_y * fixed_dt;
            transform.rotation = projectile.velocity_y.atan2(projectile.velocity_x);
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
                player_state.velocity_x = player.velocity_x;
                player_state.velocity_y = player.velocity_y;
                player_updates += 1;
            }
        }

        for (_entity, (transform, projectile)) in
            world.query::<(&Transform, &ProjectileComponent)>().iter()
        {
            projectile_updates.insert(
                projectile.id,
                (
                    transform.x,
                    transform.y,
                    projectile.velocity_x,
                    projectile.velocity_y,
                ),
            );
        }
        for projectile in projectiles.iter_mut() {
            if let Some((x, y, velocity_x, velocity_y)) =
                projectile_updates.get(&projectile.id).copied()
            {
                projectile.x = x;
                projectile.y = y;
                projectile.velocity_x = velocity_x;
                projectile.velocity_y = velocity_y;
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
