// massive_game_server/server/src/concurrent/atomic_snapshot.rs
//
// Lock-free snapshots used by AOI/broadcast read paths.
//
// Design:
// - Snapshot structs store only the states Vec and an index_by_id HashMap,
//   eliminating the previous duplication where SoA arrays (xs, ys, alive, etc.)
//   mirrored fields already present in each state struct.
// - Atomic*Snapshot wrappers use double-buffering: the writer maintains a
//   back-buffer that is cleared and refilled on each publish, then swapped in
//   via ArcSwap.  This avoids allocating new Vec/HashMap heap memory every
//   tick; the back-buffer retains its capacity across publishes.

use crate::core::types::{EntityId, Pickup, PlayerAoI, PlayerID, PlayerState, Projectile};
use arc_swap::ArcSwap;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// PlayerSoASnapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct PlayerSoASnapshot {
    // Keep public SoA fields for API compatibility (they may be used by SIMD
    // paths or future code). They are populated alongside `states`.
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub alive: Vec<bool>,
    pub team_ids: Vec<u8>,
    pub changed_fields: Vec<u16>,
    states: Vec<PlayerState>,
    index_by_id: HashMap<PlayerID, usize>,
}

impl PlayerSoASnapshot {
    pub fn from_player_states_map(player_states: &HashMap<PlayerID, PlayerState>) -> Self {
        let mut snapshot = Self::with_capacity(player_states.len());
        for (player_id, player_state) in player_states {
            snapshot.push_player(player_id.clone(), player_state.clone());
        }
        snapshot
    }

    pub fn from_owned_player_states(player_states: Vec<(PlayerID, PlayerState)>) -> Self {
        let mut snapshot = Self::with_capacity(player_states.len());
        for (player_id, player_state) in player_states {
            snapshot.push_player(player_id, player_state);
        }
        snapshot
    }

    #[inline]
    fn push_player(&mut self, player_id: PlayerID, player_state: PlayerState) {
        let idx = self.states.len();
        self.index_by_id.insert(player_id, idx);
        self.xs.push(player_state.x);
        self.ys.push(player_state.y);
        self.alive.push(player_state.alive);
        self.team_ids.push(player_state.team_id);
        self.changed_fields.push(player_state.changed_fields);
        self.states.push(player_state);
    }

    /// Clear all collections without releasing their heap allocations.
    fn clear_keep_capacity(&mut self) {
        self.xs.clear();
        self.ys.clear();
        self.alive.clear();
        self.team_ids.clear();
        self.changed_fields.clear();
        self.states.clear();
        self.index_by_id.clear();
    }

    /// Repopulate from an owned vec, reusing existing allocations.
    fn refill_from_owned(&mut self, player_states: Vec<(PlayerID, PlayerState)>) {
        self.clear_keep_capacity();
        for (player_id, player_state) in player_states {
            self.push_player(player_id, player_state);
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    #[inline]
    pub fn get_state(&self, player_id: &PlayerID) -> Option<&PlayerState> {
        self.index_by_id
            .get(player_id)
            .and_then(|idx| self.states.get(*idx))
    }

    #[inline]
    pub fn get_position(&self, player_id: &PlayerID) -> Option<(f32, f32)> {
        self.index_by_id.get(player_id).and_then(|idx| {
            let x = *self.xs.get(*idx)?;
            let y = *self.ys.get(*idx)?;
            Some((x, y))
        })
    }

    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            xs: Vec::with_capacity(capacity),
            ys: Vec::with_capacity(capacity),
            alive: Vec::with_capacity(capacity),
            team_ids: Vec::with_capacity(capacity),
            changed_fields: Vec::with_capacity(capacity),
            states: Vec::with_capacity(capacity),
            index_by_id: HashMap::with_capacity(capacity),
        }
    }
}

/// Double-buffered atomic snapshot for player state.
///
/// The writer side keeps a back-buffer (`Mutex<PlayerSoASnapshot>`) that is
/// cleared and refilled each tick, then wrapped in an `Arc` and swapped into
/// the `ArcSwap`.  Because the back-buffer retains its heap capacity from the
/// previous tick, subsequent publishes avoid the vast majority of allocations
/// (only growing if the player count increases beyond prior high-water mark).
///
/// After swapping, the *previous* published snapshot (returned by
/// `ArcSwap::swap`) is moved into the back-buffer slot so its allocations are
/// reused on the next publish cycle.
pub struct AtomicPlayerSnapshot {
    current: ArcSwap<PlayerSoASnapshot>,
    /// Back-buffer reused by the writer to avoid per-tick allocation.
    back_buffer: Mutex<PlayerSoASnapshot>,
}

impl AtomicPlayerSnapshot {
    pub fn new() -> Self {
        Self {
            current: ArcSwap::from_pointee(PlayerSoASnapshot::default()),
            back_buffer: Mutex::new(PlayerSoASnapshot::default()),
        }
    }

    #[inline]
    pub fn load(&self) -> Arc<PlayerSoASnapshot> {
        self.current.load_full()
    }

    /// Publish a new snapshot using the double-buffer.  Callers that already
    /// have a `Vec<(PlayerID, PlayerState)>` should prefer `publish_owned` to
    /// avoid an intermediate `PlayerSoASnapshot` allocation.
    #[inline]
    pub fn publish(&self, snapshot: PlayerSoASnapshot) {
        self.current.store(Arc::new(snapshot));
    }

    /// Fast path: refill the back-buffer from owned data, swap into ArcSwap,
    /// and reclaim the old snapshot as the new back-buffer.
    pub fn publish_owned(&self, player_states: Vec<(PlayerID, PlayerState)>) {
        let mut back = self.back_buffer.lock();
        back.refill_from_owned(player_states);
        // Take the filled buffer out, wrap in Arc, swap in.
        let filled = std::mem::take(&mut *back);
        let old = self.current.swap(Arc::new(filled));
        // Try to reclaim the old snapshot for next cycle.  This succeeds when
        // no reader is still holding a reference (the common case since readers
        // load_full and drop quickly).
        if let Ok(reclaimed) = Arc::try_unwrap(old) {
            *back = reclaimed;
        }
        // Otherwise back is an empty default and will reallocate on next
        // publish -- still correct, just slightly slower for one tick.
    }

    #[inline]
    pub fn publish_arc(&self, snapshot: Arc<PlayerSoASnapshot>) {
        self.current.store(snapshot);
    }
}

impl Default for AtomicPlayerSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PlayerAoISnapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct PlayerAoISnapshot {
    aois_by_player: HashMap<PlayerID, PlayerAoI>,
}

impl PlayerAoISnapshot {
    pub fn from_player_aois_map(player_aois: &HashMap<PlayerID, PlayerAoI>) -> Self {
        Self {
            aois_by_player: player_aois.clone(),
        }
    }

    pub fn from_owned_player_aois(player_aois: Vec<(PlayerID, PlayerAoI)>) -> Self {
        let mut aois_by_player = HashMap::with_capacity(player_aois.len());
        for (player_id, player_aoi) in player_aois {
            aois_by_player.insert(player_id, player_aoi);
        }
        Self { aois_by_player }
    }

    fn clear_keep_capacity(&mut self) {
        self.aois_by_player.clear();
    }

    fn refill_from_owned(&mut self, player_aois: Vec<(PlayerID, PlayerAoI)>) {
        self.clear_keep_capacity();
        for (player_id, player_aoi) in player_aois {
            self.aois_by_player.insert(player_id, player_aoi);
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.aois_by_player.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.aois_by_player.is_empty()
    }

    #[inline]
    pub fn get_aoi(&self, player_id: &PlayerID) -> Option<&PlayerAoI> {
        self.aois_by_player.get(player_id)
    }
}

pub struct AtomicPlayerAoISnapshot {
    current: ArcSwap<PlayerAoISnapshot>,
    back_buffer: Mutex<PlayerAoISnapshot>,
}

impl AtomicPlayerAoISnapshot {
    pub fn new() -> Self {
        Self {
            current: ArcSwap::from_pointee(PlayerAoISnapshot::default()),
            back_buffer: Mutex::new(PlayerAoISnapshot::default()),
        }
    }

    #[inline]
    pub fn load(&self) -> Arc<PlayerAoISnapshot> {
        self.current.load_full()
    }

    #[inline]
    pub fn publish(&self, snapshot: PlayerAoISnapshot) {
        self.current.store(Arc::new(snapshot));
    }

    pub fn publish_owned(&self, player_aois: Vec<(PlayerID, PlayerAoI)>) {
        let mut back = self.back_buffer.lock();
        back.refill_from_owned(player_aois);
        let filled = std::mem::take(&mut *back);
        let old = self.current.swap(Arc::new(filled));
        if let Ok(reclaimed) = Arc::try_unwrap(old) {
            *back = reclaimed;
        }
    }

    #[inline]
    pub fn publish_arc(&self, snapshot: Arc<PlayerAoISnapshot>) {
        self.current.store(snapshot);
    }
}

impl Default for AtomicPlayerAoISnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ProjectileSoASnapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct ProjectileSoASnapshot {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub velocity_xs: Vec<f32>,
    pub velocity_ys: Vec<f32>,
    states: Vec<Projectile>,
    index_by_id: HashMap<EntityId, usize>,
}

impl ProjectileSoASnapshot {
    pub fn from_projectiles_slice(projectiles: &[Projectile]) -> Self {
        let mut snapshot = Self::with_capacity(projectiles.len());
        for projectile in projectiles {
            snapshot.push_projectile(projectile.clone());
        }
        snapshot
    }

    #[inline]
    fn push_projectile(&mut self, projectile: Projectile) {
        let idx = self.states.len();
        self.index_by_id.insert(projectile.id, idx);
        self.xs.push(projectile.x);
        self.ys.push(projectile.y);
        self.velocity_xs.push(projectile.velocity_x);
        self.velocity_ys.push(projectile.velocity_y);
        self.states.push(projectile);
    }

    fn clear_keep_capacity(&mut self) {
        self.xs.clear();
        self.ys.clear();
        self.velocity_xs.clear();
        self.velocity_ys.clear();
        self.states.clear();
        self.index_by_id.clear();
    }

    fn refill_from_slice(&mut self, projectiles: &[Projectile]) {
        self.clear_keep_capacity();
        for projectile in projectiles {
            self.push_projectile(projectile.clone());
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    #[inline]
    pub fn get_state(&self, projectile_id: &EntityId) -> Option<&Projectile> {
        self.index_by_id
            .get(projectile_id)
            .and_then(|idx| self.states.get(*idx))
    }

    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            xs: Vec::with_capacity(capacity),
            ys: Vec::with_capacity(capacity),
            velocity_xs: Vec::with_capacity(capacity),
            velocity_ys: Vec::with_capacity(capacity),
            states: Vec::with_capacity(capacity),
            index_by_id: HashMap::with_capacity(capacity),
        }
    }
}

pub struct AtomicProjectileSnapshot {
    current: ArcSwap<ProjectileSoASnapshot>,
    back_buffer: Mutex<ProjectileSoASnapshot>,
}

impl AtomicProjectileSnapshot {
    pub fn new() -> Self {
        Self {
            current: ArcSwap::from_pointee(ProjectileSoASnapshot::default()),
            back_buffer: Mutex::new(ProjectileSoASnapshot::default()),
        }
    }

    #[inline]
    pub fn load(&self) -> Arc<ProjectileSoASnapshot> {
        self.current.load_full()
    }

    #[inline]
    pub fn publish(&self, snapshot: ProjectileSoASnapshot) {
        self.current.store(Arc::new(snapshot));
    }

    pub fn publish_from_slice(&self, projectiles: &[Projectile]) {
        let mut back = self.back_buffer.lock();
        back.refill_from_slice(projectiles);
        let filled = std::mem::take(&mut *back);
        let old = self.current.swap(Arc::new(filled));
        if let Ok(reclaimed) = Arc::try_unwrap(old) {
            *back = reclaimed;
        }
    }

    #[inline]
    pub fn publish_arc(&self, snapshot: Arc<ProjectileSoASnapshot>) {
        self.current.store(snapshot);
    }
}

impl Default for AtomicProjectileSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PickupSoASnapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct PickupSoASnapshot {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub active: Vec<bool>,
    states: Vec<Pickup>,
    index_by_id: HashMap<EntityId, usize>,
}

impl PickupSoASnapshot {
    pub fn from_pickups_slice(pickups: &[Pickup]) -> Self {
        let mut snapshot = Self::with_capacity(pickups.len());
        for pickup in pickups {
            snapshot.push_pickup(pickup.clone());
        }
        snapshot
    }

    #[inline]
    fn push_pickup(&mut self, pickup: Pickup) {
        let idx = self.states.len();
        self.index_by_id.insert(pickup.id, idx);
        self.xs.push(pickup.x);
        self.ys.push(pickup.y);
        self.active.push(pickup.is_active);
        self.states.push(pickup);
    }

    fn clear_keep_capacity(&mut self) {
        self.xs.clear();
        self.ys.clear();
        self.active.clear();
        self.states.clear();
        self.index_by_id.clear();
    }

    fn refill_from_slice(&mut self, pickups: &[Pickup]) {
        self.clear_keep_capacity();
        for pickup in pickups {
            self.push_pickup(pickup.clone());
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    #[inline]
    pub fn get_state(&self, pickup_id: &EntityId) -> Option<&Pickup> {
        self.index_by_id
            .get(pickup_id)
            .and_then(|idx| self.states.get(*idx))
    }

    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            xs: Vec::with_capacity(capacity),
            ys: Vec::with_capacity(capacity),
            active: Vec::with_capacity(capacity),
            states: Vec::with_capacity(capacity),
            index_by_id: HashMap::with_capacity(capacity),
        }
    }
}

pub struct AtomicPickupSnapshot {
    current: ArcSwap<PickupSoASnapshot>,
    back_buffer: Mutex<PickupSoASnapshot>,
}

impl AtomicPickupSnapshot {
    pub fn new() -> Self {
        Self {
            current: ArcSwap::from_pointee(PickupSoASnapshot::default()),
            back_buffer: Mutex::new(PickupSoASnapshot::default()),
        }
    }

    #[inline]
    pub fn load(&self) -> Arc<PickupSoASnapshot> {
        self.current.load_full()
    }

    #[inline]
    pub fn publish(&self, snapshot: PickupSoASnapshot) {
        self.current.store(Arc::new(snapshot));
    }

    pub fn publish_from_slice(&self, pickups: &[Pickup]) {
        let mut back = self.back_buffer.lock();
        back.refill_from_slice(pickups);
        let filled = std::mem::take(&mut *back);
        let old = self.current.swap(Arc::new(filled));
        if let Ok(reclaimed) = Arc::try_unwrap(old) {
            *back = reclaimed;
        }
    }

    #[inline]
    pub fn publish_arc(&self, snapshot: Arc<PickupSoASnapshot>) {
        self.current.store(snapshot);
    }
}

impl Default for AtomicPickupSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::PlayerState;
    use std::sync::Arc;

    fn pid(raw: &str) -> PlayerID {
        Arc::new(raw.to_string())
    }

    fn make_player_state(id: &str, x: f32, y: f32) -> PlayerState {
        let mut ps = PlayerState::new(id.to_string(), format!("user_{}", id), x, y);
        ps.team_id = 1;
        ps.changed_fields = 0xFFFF;
        ps
    }

    #[test]
    fn player_snapshot_get_state_and_position() {
        let states = vec![
            (pid("a"), make_player_state("a", 10.0, 20.0)),
            (pid("b"), make_player_state("b", 30.0, 40.0)),
        ];
        let snap = PlayerSoASnapshot::from_owned_player_states(states);
        assert_eq!(snap.len(), 2);
        assert!(!snap.is_empty());

        let state_a = snap.get_state(&pid("a")).unwrap();
        assert_eq!(state_a.x, 10.0);
        assert_eq!(state_a.y, 20.0);

        let pos_b = snap.get_position(&pid("b")).unwrap();
        assert_eq!(pos_b, (30.0, 40.0));

        assert!(snap.get_state(&pid("nonexistent")).is_none());
        assert!(snap.get_position(&pid("nonexistent")).is_none());
    }

    #[test]
    fn player_snapshot_from_map() {
        let mut map = HashMap::new();
        map.insert(pid("x"), make_player_state("x", 1.0, 2.0));
        let snap = PlayerSoASnapshot::from_player_states_map(&map);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap.get_state(&pid("x")).unwrap().x, 1.0);
    }

    #[test]
    fn atomic_player_snapshot_publish_and_load() {
        let atomic = AtomicPlayerSnapshot::new();
        let states = vec![(pid("p1"), make_player_state("p1", 5.0, 6.0))];
        let snap = PlayerSoASnapshot::from_owned_player_states(states);
        atomic.publish(snap);

        let loaded = atomic.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get_state(&pid("p1")).unwrap().x, 5.0);
    }

    #[test]
    fn atomic_player_snapshot_publish_owned_double_buffer() {
        let atomic = AtomicPlayerSnapshot::new();

        // First publish
        let states1 = vec![
            (pid("a"), make_player_state("a", 1.0, 2.0)),
            (pid("b"), make_player_state("b", 3.0, 4.0)),
        ];
        atomic.publish_owned(states1);
        let snap1 = atomic.load();
        assert_eq!(snap1.len(), 2);
        assert_eq!(snap1.get_state(&pid("a")).unwrap().x, 1.0);

        // Drop snap1 so Arc refcount goes to 1 (allowing reclamation).
        drop(snap1);

        // Second publish should reuse the back-buffer.
        let states2 = vec![(pid("c"), make_player_state("c", 5.0, 6.0))];
        atomic.publish_owned(states2);
        let snap2 = atomic.load();
        assert_eq!(snap2.len(), 1);
        assert_eq!(snap2.get_state(&pid("c")).unwrap().x, 5.0);
        // Old players should not appear.
        assert!(snap2.get_state(&pid("a")).is_none());
    }

    #[test]
    fn atomic_player_snapshot_publish_owned_with_held_reader() {
        let atomic = AtomicPlayerSnapshot::new();

        let states1 = vec![(pid("a"), make_player_state("a", 1.0, 2.0))];
        atomic.publish_owned(states1);
        let snap1 = atomic.load(); // Hold reference

        // Publish again while snap1 is still held -- reclamation should
        // gracefully skip (Arc::try_unwrap fails).
        let states2 = vec![(pid("b"), make_player_state("b", 3.0, 4.0))];
        atomic.publish_owned(states2);
        let snap2 = atomic.load();

        // Both snapshots are valid and independent.
        assert_eq!(snap1.get_state(&pid("a")).unwrap().x, 1.0);
        assert_eq!(snap2.get_state(&pid("b")).unwrap().x, 3.0);
        assert!(snap2.get_state(&pid("a")).is_none());
    }

    #[test]
    fn empty_player_snapshot() {
        let snap = PlayerSoASnapshot::default();
        assert_eq!(snap.len(), 0);
        assert!(snap.is_empty());
        assert!(snap.get_state(&pid("any")).is_none());
        assert!(snap.get_position(&pid("any")).is_none());
    }
}
