// massive_game_server/server/src/concurrent/spatial_index.rs
//
// Spatial indexing with grid cells and optional quadtree overlay.
//
// Quadtree rebuild is lock-free for readers: new trees are built in a local
// allocation and then swapped in via ArcSwap, so query paths never block
// waiting for a rebuild to finish.

use crate::core::simd;
use crate::core::types::{EntityId, PlayerID};
use crate::operational::monitoring::metrics;
use arc_swap::ArcSwap;
use dashmap::{DashMap, DashSet};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Debug, Clone, Copy)]
struct Aabb {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl Aabb {
    #[inline]
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    #[inline]
    fn intersects_circle(&self, cx: f32, cy: f32, radius: f32) -> bool {
        let clamped_x = cx.clamp(self.min_x, self.max_x);
        let clamped_y = cy.clamp(self.min_y, self.max_y);
        let dx = cx - clamped_x;
        let dy = cy - clamped_y;
        dx * dx + dy * dy <= radius * radius
    }
}

#[derive(Debug, Clone)]
struct QuadtreePoint<T> {
    id: T,
    x: f32,
    y: f32,
}

#[derive(Debug, Clone)]
struct QuadtreeNode<T> {
    bounds: Aabb,
    depth: u8,
    points: Vec<QuadtreePoint<T>>,
    children: Option<[Box<QuadtreeNode<T>>; 4]>,
}

impl<T: Clone> QuadtreeNode<T> {
    fn new(bounds: Aabb, depth: u8) -> Self {
        Self {
            bounds,
            depth,
            points: Vec::new(),
            children: None,
        }
    }

    #[inline]
    fn child_index_for(&self, x: f32, y: f32) -> usize {
        let mid_x = (self.bounds.min_x + self.bounds.max_x) * 0.5;
        let mid_y = (self.bounds.min_y + self.bounds.max_y) * 0.5;
        let right = x > mid_x;
        let top = y > mid_y;
        match (right, top) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }

    fn split(&mut self) {
        let mid_x = (self.bounds.min_x + self.bounds.max_x) * 0.5;
        let mid_y = (self.bounds.min_y + self.bounds.max_y) * 0.5;
        let depth = self.depth + 1;

        self.children = Some([
            Box::new(QuadtreeNode::new(
                Aabb {
                    min_x: self.bounds.min_x,
                    max_x: mid_x,
                    min_y: self.bounds.min_y,
                    max_y: mid_y,
                },
                depth,
            )),
            Box::new(QuadtreeNode::new(
                Aabb {
                    min_x: mid_x,
                    max_x: self.bounds.max_x,
                    min_y: self.bounds.min_y,
                    max_y: mid_y,
                },
                depth,
            )),
            Box::new(QuadtreeNode::new(
                Aabb {
                    min_x: self.bounds.min_x,
                    max_x: mid_x,
                    min_y: mid_y,
                    max_y: self.bounds.max_y,
                },
                depth,
            )),
            Box::new(QuadtreeNode::new(
                Aabb {
                    min_x: mid_x,
                    max_x: self.bounds.max_x,
                    min_y: mid_y,
                    max_y: self.bounds.max_y,
                },
                depth,
            )),
        ]);

        let existing_points = std::mem::take(&mut self.points);
        for point in existing_points {
            self.insert_into_children(point, 8, 8);
        }
    }

    fn insert_into_children(&mut self, point: QuadtreePoint<T>, capacity: usize, max_depth: u8) {
        let idx = self.child_index_for(point.x, point.y);
        if let Some(children) = self.children.as_mut() {
            let inserted = children[idx].insert(point.clone(), capacity, max_depth);
            if !inserted {
                // Fallback safety path for precision edge-cases.
                self.points.push(point);
            }
        }
    }

    fn insert(&mut self, point: QuadtreePoint<T>, capacity: usize, max_depth: u8) -> bool {
        if !self.bounds.contains(point.x, point.y) {
            return false;
        }

        if self.children.is_none() && (self.points.len() < capacity || self.depth >= max_depth) {
            self.points.push(point);
            return true;
        }

        if self.children.is_none() {
            self.split();
        }

        self.insert_into_children(point, capacity, max_depth);
        true
    }

    fn query_circle(&self, x: f32, y: f32, radius: f32, out: &mut Vec<T>) {
        if !self.bounds.intersects_circle(x, y, radius) {
            return;
        }

        let radius_sq = radius * radius;
        for point in &self.points {
            let dx = point.x - x;
            let dy = point.y - y;
            if dx * dx + dy * dy <= radius_sq {
                out.push(point.id.clone());
            }
        }

        if let Some(children) = self.children.as_ref() {
            for child in children {
                child.query_circle(x, y, radius, out);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PointQuadtree<T> {
    root: QuadtreeNode<T>,
}

impl<T: Clone> PointQuadtree<T> {
    fn from_points(
        bounds: Aabb,
        points: &[QuadtreePoint<T>],
        capacity: usize,
        max_depth: u8,
    ) -> Self {
        let mut root = QuadtreeNode::new(bounds, 0);
        for point in points {
            let _ = root.insert(point.clone(), capacity.max(2), max_depth.max(2));
        }
        Self { root }
    }

    fn query_circle(&self, x: f32, y: f32, radius: f32) -> Vec<T> {
        let mut out = Vec::new();
        self.root.query_circle(x, y, radius, &mut out);
        out
    }
}

/// Pair of quadtree indices swapped in atomically via ArcSwap so readers are
/// never blocked during a rebuild.
#[derive(Debug, Clone, Default)]
struct QuadtreeIndices {
    player_tree: Option<PointQuadtree<PlayerID>>,
    projectile_tree: Option<PointQuadtree<EntityId>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpatialQueryMode {
    Grid,
    Quadtree,
    Hybrid,
}

thread_local! {
    static CELL_QUERY_INDICES_SCRATCH: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static PLAYER_QUERY_DEDUPE_SCRATCH: RefCell<HashSet<PlayerID>> = RefCell::new(HashSet::new());
    static PROJECTILE_QUERY_DEDUPE_SCRATCH: RefCell<HashSet<EntityId>> = RefCell::new(HashSet::new());
    static PLAYER_CANDIDATE_SCRATCH: RefCell<Vec<PlayerID>> = const { RefCell::new(Vec::new()) };
    static PROJECTILE_CANDIDATE_SCRATCH: RefCell<Vec<EntityId>> = const { RefCell::new(Vec::new()) };
}

#[inline]
fn spatial_clock_origin() -> &'static Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now)
}

#[inline]
fn monotonic_now_ms() -> u64 {
    spatial_clock_origin()
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub struct ImprovedSpatialIndex {
    player_cell_members: Vec<DashSet<PlayerID>>,
    projectile_cell_members: Vec<DashSet<EntityId>>,
    grid_width: usize,
    grid_height: usize,
    cell_size: f32,
    world_min_x: f32,
    world_min_y: f32,

    // Position tracking for fast lookups
    player_positions: Arc<DashMap<PlayerID, (f32, f32)>>,
    projectile_positions: Arc<DashMap<EntityId, (f32, f32)>>,

    // Cell index tracking for efficient updates
    player_cells: Arc<DashMap<PlayerID, usize>>,
    projectile_cells: Arc<DashMap<EntityId, usize>>,

    // Hierarchical index snapshots (rebuilt at a bounded cadence).
    // Uses ArcSwap so readers never block during a rebuild.
    query_mode: SpatialQueryMode,
    quadtree_min_entities: usize,
    quadtree_rebuild_interval: Duration,
    quadtree_last_rebuild_ms: AtomicU64,
    /// Flag to prevent concurrent rebuilds.  Only one thread should build at a
    /// time; others skip if a rebuild is already in progress.
    quadtree_rebuilding: AtomicBool,
    quadtree_indices: ArcSwap<QuadtreeIndices>,
    world_bounds: Aabb,
}

impl ImprovedSpatialIndex {
    pub fn new(
        world_width: f32,
        world_height: f32,
        world_min_x: f32,
        world_min_y: f32,
        cell_size: f32,
    ) -> Self {
        assert!(
            cell_size.is_finite() && cell_size > 0.0,
            "cell_size must be finite and > 0.0"
        );
        let grid_width = ((world_width / cell_size).ceil() as usize).max(1);
        let grid_height = ((world_height / cell_size).ceil() as usize).max(1);
        let total_cells = grid_width * grid_height;

        let mut player_cell_members = Vec::with_capacity(total_cells);
        let mut projectile_cell_members = Vec::with_capacity(total_cells);
        for _ in 0..total_cells {
            player_cell_members.push(DashSet::new());
            projectile_cell_members.push(DashSet::new());
        }

        let query_mode = parse_query_mode_from_env();
        let quadtree_min_entities = std::env::var("MGS_SPATIAL_QUADTREE_MIN_ENTITIES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(64)
            .max(8);
        let quadtree_rebuild_interval_ms = std::env::var("MGS_SPATIAL_QUADTREE_REBUILD_MS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(33)
            .max(8);
        let initial_rebuild_ms = monotonic_now_ms().saturating_sub(quadtree_rebuild_interval_ms);

        debug!(
            "Spatial index initialized: {}x{} grid, {} total cells, cell size: {}, mode={:?}, quadtree_min_entities={}, quadtree_rebuild_ms={}",
            grid_width,
            grid_height,
            total_cells,
            cell_size,
            query_mode,
            quadtree_min_entities,
            quadtree_rebuild_interval_ms,
        );

        ImprovedSpatialIndex {
            player_cell_members,
            projectile_cell_members,
            grid_width,
            grid_height,
            cell_size,
            world_min_x,
            world_min_y,
            player_positions: Arc::new(DashMap::new()),
            projectile_positions: Arc::new(DashMap::new()),
            player_cells: Arc::new(DashMap::new()),
            projectile_cells: Arc::new(DashMap::new()),
            query_mode,
            quadtree_min_entities,
            quadtree_rebuild_interval: Duration::from_millis(quadtree_rebuild_interval_ms),
            quadtree_last_rebuild_ms: AtomicU64::new(initial_rebuild_ms),
            quadtree_rebuilding: AtomicBool::new(false),
            quadtree_indices: ArcSwap::from_pointee(QuadtreeIndices::default()),
            world_bounds: Aabb {
                min_x: world_min_x,
                max_x: world_min_x + world_width,
                min_y: world_min_y,
                max_y: world_min_y + world_height,
            },
        }
    }

    #[inline]
    fn should_use_quadtree(&self, entity_count: usize) -> bool {
        match self.query_mode {
            SpatialQueryMode::Grid => false,
            SpatialQueryMode::Quadtree => true,
            SpatialQueryMode::Hybrid => entity_count >= self.quadtree_min_entities,
        }
    }

    /// Rebuild quadtree indices if enough time has elapsed since the last
    /// rebuild.  The new trees are built in local allocations and then swapped
    /// in atomically via ArcSwap, so concurrent readers are never blocked.
    fn maybe_rebuild_quadtrees(&self) {
        if self.query_mode == SpatialQueryMode::Grid {
            return;
        }

        let rebuild_interval_ms = self
            .quadtree_rebuild_interval
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let now_ms = monotonic_now_ms();
        let last_rebuild_ms = self.quadtree_last_rebuild_ms.load(Ordering::Acquire);
        if now_ms.saturating_sub(last_rebuild_ms) < rebuild_interval_ms {
            return;
        }

        // Try to claim the rebuild slot.  If another thread is already
        // rebuilding, we skip and use the stale tree (still valid, just not
        // up-to-date).
        if self
            .quadtree_rebuilding
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        // Double-check the timestamp under the flag (another thread may have
        // finished a rebuild between the first check and our CAS).
        let refreshed_last_rebuild_ms = self.quadtree_last_rebuild_ms.load(Ordering::Acquire);
        if now_ms.saturating_sub(refreshed_last_rebuild_ms) < rebuild_interval_ms {
            self.quadtree_rebuilding.store(false, Ordering::Release);
            return;
        }
        self.quadtree_last_rebuild_ms
            .store(now_ms, Ordering::Release);

        let rebuild_start = Instant::now();

        // Build new trees in local memory (no locks held by readers).
        let mut player_points = Vec::with_capacity(self.player_positions.len());
        for entry in self.player_positions.iter() {
            let (x, y) = *entry.value();
            player_points.push(QuadtreePoint {
                id: entry.key().clone(),
                x,
                y,
            });
        }
        let mut projectile_points = Vec::with_capacity(self.projectile_positions.len());
        for entry in self.projectile_positions.iter() {
            let (x, y) = *entry.value();
            projectile_points.push(QuadtreePoint {
                id: *entry.key(),
                x,
                y,
            });
        }

        let player_tree = if player_points.is_empty() {
            None
        } else {
            Some(PointQuadtree::from_points(
                self.world_bounds,
                &player_points,
                12,
                8,
            ))
        };
        let projectile_tree = if projectile_points.is_empty() {
            None
        } else {
            Some(PointQuadtree::from_points(
                self.world_bounds,
                &projectile_points,
                12,
                8,
            ))
        };

        // Atomic swap -- readers loading the ArcSwap see either the old or the
        // new tree, never a partially-built tree, and are never blocked.
        self.quadtree_indices.store(Arc::new(QuadtreeIndices {
            player_tree,
            projectile_tree,
        }));

        metrics::record_spatial_index_rebuild(rebuild_start.elapsed().as_secs_f64());

        self.quadtree_rebuilding.store(false, Ordering::Release);
    }

    #[inline]
    fn get_cell_index(&self, x: f32, y: f32) -> usize {
        let grid_x = ((x - self.world_min_x) / self.cell_size).floor().max(0.0) as usize;
        let grid_y = ((y - self.world_min_y) / self.cell_size).floor().max(0.0) as usize;

        let clamped_x = grid_x.min(self.grid_width.saturating_sub(1));
        let clamped_y = grid_y.min(self.grid_height.saturating_sub(1));

        clamped_y * self.grid_width + clamped_x
    }

    #[inline]
    fn fill_cells_in_radius(
        &self,
        center_x: f32,
        center_y: f32,
        radius: f32,
        out: &mut Vec<usize>,
    ) {
        let min_x = center_x - radius;
        let max_x = center_x + radius;
        let min_y = center_y - radius;
        let max_y = center_y + radius;

        let min_grid_x = ((min_x - self.world_min_x) / self.cell_size)
            .floor()
            .max(0.0) as usize;
        let max_grid_x = ((max_x - self.world_min_x) / self.cell_size)
            .ceil()
            .min(self.grid_width as f32) as usize;
        let min_grid_y = ((min_y - self.world_min_y) / self.cell_size)
            .floor()
            .max(0.0) as usize;
        let max_grid_y = ((max_y - self.world_min_y) / self.cell_size)
            .ceil()
            .min(self.grid_height as f32) as usize;

        out.clear();
        for y in min_grid_y..max_grid_y {
            for x in min_grid_x..max_grid_x {
                if x < self.grid_width && y < self.grid_height {
                    out.push(y * self.grid_width + x);
                }
            }
        }
    }

    fn collect_grid_player_candidates(&self, x: f32, y: f32, radius: f32) -> Vec<PlayerID> {
        CELL_QUERY_INDICES_SCRATCH.with(|scratch| {
            let mut cell_indices = scratch.borrow_mut();
            self.fill_cells_in_radius(x, y, radius, &mut cell_indices);
            PLAYER_QUERY_DEDUPE_SCRATCH.with(|dedupe_scratch| {
                let mut checked_players = dedupe_scratch.borrow_mut();
                checked_players.clear();

                PLAYER_CANDIDATE_SCRATCH.with(|candidate_scratch| {
                    let mut candidate_ids = candidate_scratch.borrow_mut();
                    candidate_ids.clear();
                    for cell_idx in cell_indices.iter().copied() {
                        if let Some(cell) = self.player_cell_members.get(cell_idx) {
                            for player_id in cell.iter() {
                                if checked_players.insert(player_id.key().clone()) {
                                    candidate_ids.push(player_id.key().clone());
                                }
                            }
                        }
                    }
                    // Return owned copy; scratch buffer retains capacity for next call
                    candidate_ids.clone()
                })
            })
        })
    }

    fn collect_grid_projectile_candidates(&self, x: f32, y: f32, radius: f32) -> Vec<EntityId> {
        CELL_QUERY_INDICES_SCRATCH.with(|scratch| {
            let mut cell_indices = scratch.borrow_mut();
            self.fill_cells_in_radius(x, y, radius, &mut cell_indices);
            PROJECTILE_QUERY_DEDUPE_SCRATCH.with(|dedupe_scratch| {
                let mut checked_projectiles = dedupe_scratch.borrow_mut();
                checked_projectiles.clear();

                PROJECTILE_CANDIDATE_SCRATCH.with(|candidate_scratch| {
                    let mut candidate_ids = candidate_scratch.borrow_mut();
                    candidate_ids.clear();
                    for cell_idx in cell_indices.iter().copied() {
                        if let Some(cell) = self.projectile_cell_members.get(cell_idx) {
                            for projectile_id in cell.iter() {
                                if checked_projectiles.insert(*projectile_id.key()) {
                                    candidate_ids.push(*projectile_id.key());
                                }
                            }
                        }
                    }
                    candidate_ids.clone()
                })
            })
        })
    }

    // Player methods
    pub fn update_player_position(&self, player_id: PlayerID, x: f32, y: f32) {
        let new_cell_idx = self.get_cell_index(x, y);

        // Check if player moved to a different cell
        let old_cell_idx = self
            .player_cells
            .get(&player_id)
            .map(|entry| *entry.value());

        if let Some(old_idx) = old_cell_idx {
            if old_idx != new_cell_idx {
                if let Some(new_cell) = self.player_cell_members.get(new_cell_idx) {
                    new_cell.insert(player_id.clone());
                }
                self.player_cells.insert(player_id.clone(), new_cell_idx);
                if let Some(old_cell) = self.player_cell_members.get(old_idx) {
                    old_cell.remove(&player_id);
                }
            }
        } else {
            // First time tracking this player
            if let Some(new_cell) = self.player_cell_members.get(new_cell_idx) {
                new_cell.insert(player_id.clone());
            }
            self.player_cells.insert(player_id.clone(), new_cell_idx);
        }

        // Always update position
        self.player_positions.insert(player_id, (x, y));
    }

    pub fn remove_player(&self, player_id: &PlayerID) {
        if let Some((_, cell_idx)) = self.player_cells.remove(player_id) {
            if let Some(cell) = self.player_cell_members.get(cell_idx) {
                cell.remove(player_id);
            }
        }
        self.player_positions.remove(player_id);
    }

    pub fn query_nearby_players(&self, x: f32, y: f32, radius: f32) -> Vec<PlayerID> {
        let radius_squared = radius * radius;

        let candidate_ids: Vec<PlayerID> = if self.should_use_quadtree(self.player_positions.len())
        {
            self.maybe_rebuild_quadtrees();
            let indices = self.quadtree_indices.load();
            if let Some(tree) = indices.player_tree.as_ref() {
                tree.query_circle(x, y, radius)
            } else {
                self.collect_grid_player_candidates(x, y, radius)
            }
        } else {
            self.collect_grid_player_candidates(x, y, radius)
        };

        let mut candidate_xs = Vec::with_capacity(candidate_ids.len());
        let mut candidate_ys = Vec::with_capacity(candidate_ids.len());
        let mut filtered_ids = Vec::with_capacity(candidate_ids.len());

        for player_id in candidate_ids {
            if let Some(pos_entry) = self.player_positions.get(&player_id) {
                let (px, py) = *pos_entry.value();
                filtered_ids.push(player_id);
                candidate_xs.push(px);
                candidate_ys.push(py);
            }
        }

        let mut matched_indices = Vec::with_capacity(filtered_ids.len());
        simd::filter_indices_within_radius(
            &candidate_xs,
            &candidate_ys,
            x,
            y,
            radius_squared,
            &mut matched_indices,
        );

        let mut nearby_players = Vec::with_capacity(matched_indices.len());
        for idx in matched_indices {
            if let Some(player_id) = filtered_ids.get(idx) {
                nearby_players.push(player_id.clone());
            }
        }
        nearby_players
    }

    /// Returns nearby players together with their cached positions from the spatial index.
    /// This avoids a second map lookup in hot collision paths.
    pub fn query_nearby_players_with_positions(
        &self,
        x: f32,
        y: f32,
        radius: f32,
    ) -> Vec<(PlayerID, f32, f32)> {
        let radius_squared = radius * radius;

        let candidate_ids: Vec<PlayerID> = if self.should_use_quadtree(self.player_positions.len())
        {
            self.maybe_rebuild_quadtrees();
            let indices = self.quadtree_indices.load();
            if let Some(tree) = indices.player_tree.as_ref() {
                tree.query_circle(x, y, radius)
            } else {
                self.collect_grid_player_candidates(x, y, radius)
            }
        } else {
            self.collect_grid_player_candidates(x, y, radius)
        };

        let mut candidate_ids_filtered = Vec::with_capacity(candidate_ids.len());
        let mut candidate_xs = Vec::with_capacity(candidate_ids.len());
        let mut candidate_ys = Vec::with_capacity(candidate_ids.len());

        for player_id in candidate_ids {
            if let Some(pos_entry) = self.player_positions.get(&player_id) {
                let (px, py) = *pos_entry.value();
                candidate_ids_filtered.push(player_id);
                candidate_xs.push(px);
                candidate_ys.push(py);
            }
        }

        let mut matched_indices = Vec::with_capacity(candidate_ids_filtered.len());
        simd::filter_indices_within_radius(
            &candidate_xs,
            &candidate_ys,
            x,
            y,
            radius_squared,
            &mut matched_indices,
        );

        let mut nearby_players = Vec::with_capacity(matched_indices.len());
        for idx in matched_indices {
            if let (Some(player_id), Some(px), Some(py)) = (
                candidate_ids_filtered.get(idx),
                candidate_xs.get(idx),
                candidate_ys.get(idx),
            ) {
                nearby_players.push((player_id.clone(), *px, *py));
            }
        }
        nearby_players
    }

    // Projectile methods
    pub fn update_projectile_position(&self, proj_id: EntityId, x: f32, y: f32) {
        let new_cell_idx = self.get_cell_index(x, y);

        // Check if projectile moved to a different cell
        let old_cell_idx = self
            .projectile_cells
            .get(&proj_id)
            .map(|entry| *entry.value());

        if let Some(old_idx) = old_cell_idx {
            if old_idx != new_cell_idx {
                if let Some(old_cell) = self.projectile_cell_members.get(old_idx) {
                    old_cell.remove(&proj_id);
                }
                if let Some(new_cell) = self.projectile_cell_members.get(new_cell_idx) {
                    new_cell.insert(proj_id);
                }
                self.projectile_cells.insert(proj_id, new_cell_idx);
            }
        } else {
            // First time tracking this projectile
            if let Some(new_cell) = self.projectile_cell_members.get(new_cell_idx) {
                new_cell.insert(proj_id);
            }
            self.projectile_cells.insert(proj_id, new_cell_idx);
        }

        // Always update position
        self.projectile_positions.insert(proj_id, (x, y));
    }

    pub fn remove_projectile(&self, proj_id: &EntityId) {
        if let Some((_, cell_idx)) = self.projectile_cells.remove(proj_id) {
            if let Some(cell) = self.projectile_cell_members.get(cell_idx) {
                cell.remove(proj_id);
            }
        }
        self.projectile_positions.remove(proj_id);
    }

    pub fn query_nearby_projectiles(&self, x: f32, y: f32, radius: f32) -> Vec<EntityId> {
        let radius_squared = radius * radius;

        let candidate_ids: Vec<EntityId> =
            if self.should_use_quadtree(self.projectile_positions.len()) {
                self.maybe_rebuild_quadtrees();
                let indices = self.quadtree_indices.load();
                if let Some(tree) = indices.projectile_tree.as_ref() {
                    tree.query_circle(x, y, radius)
                } else {
                    self.collect_grid_projectile_candidates(x, y, radius)
                }
            } else {
                self.collect_grid_projectile_candidates(x, y, radius)
            };

        let mut candidate_ids_filtered = Vec::with_capacity(candidate_ids.len());
        let mut candidate_xs = Vec::with_capacity(candidate_ids.len());
        let mut candidate_ys = Vec::with_capacity(candidate_ids.len());

        for proj_id in candidate_ids {
            if let Some(pos_entry) = self.projectile_positions.get(&proj_id) {
                let (px, py) = *pos_entry.value();
                candidate_ids_filtered.push(proj_id);
                candidate_xs.push(px);
                candidate_ys.push(py);
            }
        }

        let mut matched_indices = Vec::with_capacity(candidate_ids_filtered.len());
        simd::filter_indices_within_radius(
            &candidate_xs,
            &candidate_ys,
            x,
            y,
            radius_squared,
            &mut matched_indices,
        );

        let mut nearby_projectiles = Vec::with_capacity(matched_indices.len());
        for idx in matched_indices {
            if let Some(projectile_id) = candidate_ids_filtered.get(idx) {
                nearby_projectiles.push(*projectile_id);
            }
        }
        nearby_projectiles
    }

    // Batch operations for efficiency
    pub fn batch_update_projectiles(&self, updates: &[(EntityId, f32, f32)]) {
        for &(proj_id, x, y) in updates {
            self.update_projectile_position(proj_id, x, y);
        }
    }

    pub fn get_stats(&self) -> SpatialIndexStats {
        let total_players = self.player_positions.len();
        let total_projectiles = self.projectile_positions.len();
        let mut occupied_cells = 0;
        let mut max_entities_per_cell = 0;

        for cell_idx in 0..self.player_cell_members.len() {
            let player_count = self
                .player_cell_members
                .get(cell_idx)
                .map(|cell| cell.len())
                .unwrap_or(0);
            let projectile_count = self
                .projectile_cell_members
                .get(cell_idx)
                .map(|cell| cell.len())
                .unwrap_or(0);
            let entity_count = player_count + projectile_count;
            if entity_count > 0 {
                occupied_cells += 1;
                max_entities_per_cell = max_entities_per_cell.max(entity_count);
            }
        }

        SpatialIndexStats {
            total_players,
            total_projectiles,
            occupied_cells,
            total_cells: self.player_cell_members.len(),
            max_entities_per_cell,
            query_mode: match self.query_mode {
                SpatialQueryMode::Grid => "grid",
                SpatialQueryMode::Quadtree => "quadtree",
                SpatialQueryMode::Hybrid => "hybrid",
            }
            .to_string(),
        }
    }
}

fn parse_query_mode_from_env() -> SpatialQueryMode {
    let raw = std::env::var("MGS_SPATIAL_INDEX_MODE")
        .unwrap_or_else(|_| "hybrid".to_string())
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "grid" => SpatialQueryMode::Grid,
        "quadtree" | "quad" | "hierarchical" => SpatialQueryMode::Quadtree,
        _ => SpatialQueryMode::Hybrid,
    }
}

#[derive(Debug)]
pub struct SpatialIndexStats {
    pub total_players: usize,
    pub total_projectiles: usize,
    pub occupied_cells: usize,
    pub total_cells: usize,
    pub max_entities_per_cell: usize,
    pub query_mode: String,
}

#[cfg(test)]
mod tests {
    use super::ImprovedSpatialIndex;
    use crate::core::types::{EntityId, PlayerID};
    use std::sync::Arc;

    fn pid(raw: &str) -> PlayerID {
        Arc::from(raw.to_string())
    }

    #[test]
    fn query_nearby_players_with_positions_returns_positions() {
        let index = ImprovedSpatialIndex::new(1024.0, 1024.0, 0.0, 0.0, 64.0);
        let p1 = pid("p1");
        let p2 = pid("p2");
        let p3 = pid("p3");

        index.update_player_position(p1.clone(), 10.0, 10.0);
        index.update_player_position(p2.clone(), 28.0, 24.0);
        index.update_player_position(p3, 420.0, 400.0);

        let mut nearby = index.query_nearby_players_with_positions(12.0, 12.0, 30.0);
        nearby.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));

        assert_eq!(nearby.len(), 2);
        assert_eq!(nearby[0].0.as_ref(), "p1");
        assert_eq!(nearby[0].1, 10.0);
        assert_eq!(nearby[0].2, 10.0);
        assert_eq!(nearby[1].0.as_ref(), "p2");
        assert_eq!(nearby[1].1, 28.0);
        assert_eq!(nearby[1].2, 24.0);
    }

    #[test]
    fn moving_player_between_cells_keeps_single_membership() {
        let index = ImprovedSpatialIndex::new(512.0, 512.0, 0.0, 0.0, 64.0);
        let p1 = pid("p1");

        index.update_player_position(p1.clone(), 20.0, 20.0);
        index.update_player_position(p1.clone(), 220.0, 20.0);

        let old_cell_hits = index.query_nearby_players(20.0, 20.0, 24.0);
        assert!(
            !old_cell_hits.iter().any(|id| id.as_ref() == "p1"),
            "player should have been removed from the old cell"
        );

        let new_cell_hits = index.query_nearby_players(220.0, 20.0, 24.0);
        let matches = new_cell_hits
            .iter()
            .filter(|id| id.as_ref() == "p1")
            .count();
        assert_eq!(matches, 1, "player should exist exactly once");
    }

    #[test]
    fn moving_projectile_between_cells_keeps_single_membership() {
        let index = ImprovedSpatialIndex::new(512.0, 512.0, 0.0, 0.0, 64.0);
        let projectile_id: EntityId = 77;

        index.update_projectile_position(projectile_id, 24.0, 24.0);
        index.update_projectile_position(projectile_id, 232.0, 24.0);

        let old_cell_hits = index.query_nearby_projectiles(24.0, 24.0, 24.0);
        assert!(
            !old_cell_hits.contains(&projectile_id),
            "projectile should have been removed from the old cell"
        );

        let new_cell_hits = index.query_nearby_projectiles(232.0, 24.0, 24.0);
        let matches = new_cell_hits
            .iter()
            .filter(|id| **id == projectile_id)
            .count();
        assert_eq!(matches, 1, "projectile should exist exactly once");
    }

    #[test]
    #[should_panic(expected = "cell_size must be finite and > 0.0")]
    fn new_panics_for_zero_cell_size() {
        let _ = ImprovedSpatialIndex::new(512.0, 512.0, 0.0, 0.0, 0.0);
    }
}
