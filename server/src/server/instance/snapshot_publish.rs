use super::*;

impl MassiveGameServer {
    fn publish_player_soa_snapshot_if_enabled(&self) {
        if !join_soa_snapshot_enabled() {
            return;
        }

        let mut owned_states = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                owned_states.push((player_id.clone(), player_state.clone()));
            });
        // Use double-buffered publish to avoid per-tick allocation churn.
        self.player_soa_snapshot.publish_owned(owned_states);
    }

    fn publish_entity_soa_snapshots_if_enabled(&self) {
        if !join_entity_soa_snapshot_enabled() {
            return;
        }

        let projectiles_guard = self.projectiles.read();
        // Use double-buffered publish to reuse snapshot allocations.
        self.projectile_soa_snapshot
            .publish_from_slice(&projectiles_guard);
        drop(projectiles_guard);

        let pickups_guard = self.pickups.read();
        self.pickup_soa_snapshot.publish_from_slice(&pickups_guard);
    }
    pub(super) fn publish_player_aoi_snapshot_if_enabled(&self) {
        if !join_authoritative_aoi_snapshot_enabled() {
            return;
        }

        let mut owned_aois = Vec::with_capacity(self.player_aois.len());
        for aoi_entry in self.player_aois.iter() {
            let player_id = self.player_manager.id_pool.get_or_create(aoi_entry.key());
            owned_aois.push((player_id, aoi_entry.value().clone()));
        }
        // Use double-buffered publish to reuse snapshot allocations.
        self.player_aoi_snapshot.publish_owned(owned_aois);
    }
    pub(super) fn publish_authoritative_lock_free_snapshots(&self) {
        self.publish_player_soa_snapshot_if_enabled();
        self.publish_entity_soa_snapshots_if_enabled();
        self.publish_player_aoi_snapshot_if_enabled();
    }
    pub(super) fn rebuild_player_soa_snapshot_from_authoritative_state(
        &self,
    ) -> Arc<PlayerSoASnapshot> {
        let mut owned_states = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                owned_states.push((player_id.clone(), player_state.clone()));
            });
        let snapshot = Arc::new(PlayerSoASnapshot::from_owned_player_states(owned_states));
        self.player_soa_snapshot.publish_arc(snapshot.clone());
        snapshot
    }
    pub(super) fn rebuild_projectile_soa_snapshot_from_authoritative_state(
        &self,
    ) -> Arc<ProjectileSoASnapshot> {
        let projectiles_guard = self.projectiles.read();
        let snapshot = Arc::new(ProjectileSoASnapshot::from_projectiles_slice(
            &projectiles_guard,
        ));
        drop(projectiles_guard);
        self.projectile_soa_snapshot.publish_arc(snapshot.clone());
        snapshot
    }
    pub(super) fn rebuild_pickup_soa_snapshot_from_authoritative_state(
        &self,
    ) -> Arc<PickupSoASnapshot> {
        let pickups_guard = self.pickups.read();
        let snapshot = Arc::new(PickupSoASnapshot::from_pickups_slice(&pickups_guard));
        drop(pickups_guard);
        self.pickup_soa_snapshot.publish_arc(snapshot.clone());
        snapshot
    }
}
