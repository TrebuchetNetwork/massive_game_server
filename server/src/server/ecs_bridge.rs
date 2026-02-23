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
    pub skipped_contention: bool,
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
                    .unwrap_or(true);
                if enabled {
                    EcsMode::Authoritative
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

        #[derive(Clone)]
        struct PlayerSnapshot {
            id: PlayerID,
            username: String,
            transform: Transform,
            health: i32,
            team_id: u8,
            alive: bool,
            velocity_x: f32,
            velocity_y: f32,
        }

        let mut player_snapshots = Vec::with_capacity(player_manager.player_count());
        player_manager.for_each_player(|player_id, player_state| {
            player_snapshots.push(PlayerSnapshot {
                id: player_id.clone(),
                username: player_state.username.clone(),
                transform: Transform {
                    x: player_state.x,
                    y: player_state.y,
                    rotation: player_state.rotation,
                },
                health: player_state.health,
                team_id: player_state.team_id,
                alive: player_state.alive,
                velocity_x: player_state.velocity_x,
                velocity_y: player_state.velocity_y,
            });
        });
        let player_count = player_snapshots.len();
        let projectile_snapshots: Vec<Projectile> = projectiles.to_vec();
        let pickup_snapshots: Vec<Pickup> = pickups.to_vec();

        let Some(mut world) = self.world.try_write() else {
            return EcsSnapshotStats {
                players: 0,
                projectiles: 0,
                pickups: 0,
                skipped_contention: true,
            };
        };
        let Some(mut entity_index) = self.entity_index.try_write() else {
            return EcsSnapshotStats {
                players: 0,
                projectiles: 0,
                pickups: 0,
                skipped_contention: true,
            };
        };
        let mut seen_player_ids: HashSet<PlayerID> = HashSet::with_capacity(player_snapshots.len());
        for player_snapshot in &player_snapshots {
            seen_player_ids.insert(player_snapshot.id.clone());

            let entity = entity_index
                .players
                .get(&player_snapshot.id)
                .copied()
                .filter(|entity| world.contains(*entity))
                .unwrap_or_else(|| {
                    let entity = world.spawn((
                        player_snapshot.transform.clone(),
                        PlayerComponent {
                            id: player_snapshot.id.clone(),
                            username: player_snapshot.username.clone(),
                            health: player_snapshot.health,
                            team_id: player_snapshot.team_id,
                            alive: player_snapshot.alive,
                            velocity_x: player_snapshot.velocity_x,
                            velocity_y: player_snapshot.velocity_y,
                        },
                    ));
                    entity_index
                        .players
                        .insert(player_snapshot.id.clone(), entity);
                    entity
                });

            if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                transform.x = player_snapshot.transform.x;
                transform.y = player_snapshot.transform.y;
                transform.rotation = player_snapshot.transform.rotation;
            }
            if let Ok(mut player_component) = world.get::<&mut PlayerComponent>(entity) {
                player_component.username = player_snapshot.username.clone();
                player_component.health = player_snapshot.health;
                player_component.team_id = player_snapshot.team_id;
                player_component.alive = player_snapshot.alive;
                player_component.velocity_x = player_snapshot.velocity_x;
                player_component.velocity_y = player_snapshot.velocity_y;
            }
        }

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

        let mut seen_projectile_ids: HashSet<EntityId> =
            HashSet::with_capacity(projectile_snapshots.len());
        for projectile in &projectile_snapshots {
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

        let mut seen_pickup_ids: HashSet<EntityId> = HashSet::with_capacity(pickup_snapshots.len());
        for pickup in &pickup_snapshots {
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
            projectiles: projectile_snapshots.len(),
            pickups: pickup_snapshots.len(),
            skipped_contention: false,
        }
    }

    fn run_authoritative_systems(&self, world: &mut World) {
        let fixed_dt = std::env::var("MGS_ECS_AUTHORITATIVE_FIXED_DT")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|value| *value > 0.0)
            .unwrap_or(1.0 / 60.0);

        for (_entity, (transform, player)) in
            world.query::<(&mut Transform, &PlayerComponent)>().iter()
        {
            transform.x += player.velocity_x * fixed_dt;
            transform.y += player.velocity_y * fixed_dt;
        }

        for (_entity, (transform, projectile)) in world
            .query::<(&mut Transform, &ProjectileComponent)>()
            .iter()
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

        let Some(world) = self.world.try_read() else {
            return EcsSnapshotStats {
                players: 0,
                projectiles: 0,
                pickups: 0,
                skipped_contention: true,
            };
        };
        #[derive(Clone)]
        struct PlayerReconcileUpdate {
            id: PlayerID,
            x: f32,
            y: f32,
            rotation: f32,
            health: i32,
            alive: bool,
            velocity_x: f32,
            velocity_y: f32,
        }

        let mut player_updates = Vec::new();
        let mut projectile_updates = HashMap::with_capacity(projectiles.len());
        let mut pickup_updates = HashMap::with_capacity(pickups.len());

        for (_entity, (transform, player)) in world.query::<(&Transform, &PlayerComponent)>().iter()
        {
            player_updates.push(PlayerReconcileUpdate {
                id: player.id.clone(),
                x: transform.x,
                y: transform.y,
                rotation: transform.rotation,
                health: player.health,
                alive: player.alive,
                velocity_x: player.velocity_x,
                velocity_y: player.velocity_y,
            });
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
        drop(world);

        let mut reconciled_players = 0usize;
        for update in player_updates {
            if let Some(mut player_state) = player_manager.get_player_state_mut(&update.id) {
                player_state.x = update.x;
                player_state.y = update.y;
                player_state.rotation = update.rotation;
                player_state.health = update.health;
                player_state.alive = update.alive;
                player_state.velocity_x = update.velocity_x;
                player_state.velocity_y = update.velocity_y;
                reconciled_players += 1;
            }
        }

        for pickup in pickups.iter_mut() {
            if let Some((x, y, is_active)) = pickup_updates.get(&pickup.id).copied() {
                pickup.x = x;
                pickup.y = y;
                pickup.is_active = is_active;
            }
        }

        EcsSnapshotStats {
            players: reconciled_players,
            projectiles: projectile_updates.len(),
            pickups: pickup_updates.len(),
            skipped_contention: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concurrent::spatial_index::ImprovedSpatialIndex;
    use crate::core::types::{CorePickupType, ServerWeaponType};

    fn authoritative_bridge_for_test() -> EcsBridge {
        EcsBridge {
            world: Arc::new(RwLock::new(World::new())),
            entity_index: Arc::new(RwLock::new(EcsEntityIndex::default())),
            mode: EcsMode::Authoritative,
            rebuild_stride_frames: 1,
        }
    }

    #[test]
    fn authoritative_reconciliation_applies_after_snapshot_capture() {
        let spatial_index = Arc::new(ImprovedSpatialIndex::new(
            2000.0, 2000.0, -1000.0, -1000.0, 64.0,
        ));
        let player_manager = ImprovedPlayerManager::new(8, spatial_index);
        let player_id = player_manager
            .add_player(
                "ecs_test_player".to_string(),
                "ECS Test".to_string(),
                0.0,
                0.0,
            )
            .expect("player should be created");
        {
            let mut player_state = player_manager
                .get_player_state_mut(&player_id)
                .expect("player state should exist");
            player_state.velocity_x = 60.0;
            player_state.velocity_y = 0.0;
        }

        let mut projectile = Projectile::new(
            player_id.clone(),
            ServerWeaponType::Rifle,
            10.0,
            5.0,
            1.0,
            0.0,
            1.0,
        );
        projectile.id = 7001;
        let pickup = Pickup::new(8001, -5.0, 9.0, CorePickupType::Health);

        let bridge = authoritative_bridge_for_test();
        bridge.rebuild_snapshot(
            &player_manager,
            std::slice::from_ref(&projectile),
            std::slice::from_ref(&pickup),
        );

        let mut projectiles = vec![Projectile {
            x: -999.0,
            y: -999.0,
            ..projectile.clone()
        }];
        let mut pickups = vec![Pickup {
            x: -999.0,
            y: -999.0,
            is_active: false,
            ..pickup.clone()
        }];

        let stats = bridge.apply_authoritative_reconciliation(
            &player_manager,
            &mut projectiles,
            &mut pickups,
        );

        assert!(!stats.skipped_contention);
        assert_eq!(stats.players, 1);
        assert_eq!(stats.projectiles, 1);
        assert_eq!(stats.pickups, 1);

        let reconciled_player = player_manager
            .get_player_state(&player_id)
            .expect("player should still exist");
        assert!(reconciled_player.x > 0.0);
        assert_eq!(reconciled_player.y, 0.0);
        assert_eq!(reconciled_player.velocity_x, 60.0);

        assert!(projectiles[0].x > 10.0);
        assert_eq!(projectiles[0].y, 5.0);
        assert_eq!(pickups[0].x, -5.0);
        assert_eq!(pickups[0].y, 9.0);
        assert!(pickups[0].is_active);
    }
}
