// massive_game_server/server/src/world/navigation.rs

use crate::core::types::Vec2;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Thread-local scratch buffers for `GridNav::find_path`, avoiding
/// per-call HashMap allocations.  Uses a generation counter so that
/// we can "clear" the buffers in O(1) by bumping the generation.
struct GridNavScratch {
    /// Generation counter – bumped at the start of each `find_path` call.
    generation: u32,
    /// Per-node generation stamp.  A node is "visited" iff
    /// `visited_gen[idx] == generation`.
    visited_gen: Vec<u32>,
    /// Best cost-so-far for each node (valid only when `visited_gen[idx] == generation`).
    cost_so_far: Vec<i32>,
    /// came_from parent for path reconstruction (encoded as flat index,
    /// valid only when `visited_gen[idx] == generation`).
    came_from: Vec<u32>,
}

impl GridNavScratch {
    fn new() -> Self {
        Self {
            generation: 0,
            visited_gen: Vec::new(),
            cost_so_far: Vec::new(),
            came_from: Vec::new(),
        }
    }

    /// Ensure the scratch buffers are large enough for `node_count` nodes
    /// and start a new generation (logically clears all data in O(1)).
    fn reset(&mut self, node_count: usize) {
        // Handle generation wrap-around by actually clearing.
        if self.generation == u32::MAX {
            self.visited_gen.clear();
            self.generation = 0;
        }
        self.generation += 1;
        if self.visited_gen.len() < node_count {
            self.visited_gen.resize(node_count, 0);
            self.cost_so_far.resize(node_count, 0);
            self.came_from.resize(node_count, 0);
        }
    }

    #[inline]
    fn is_visited(&self, idx: usize) -> bool {
        self.visited_gen.get(idx).copied() == Some(self.generation)
    }

    #[inline]
    fn get_cost(&self, idx: usize) -> i32 {
        if self.is_visited(idx) {
            self.cost_so_far[idx]
        } else {
            i32::MAX
        }
    }

    #[inline]
    fn set(&mut self, idx: usize, cost: i32, parent_idx: u32) {
        self.visited_gen[idx] = self.generation;
        self.cost_so_far[idx] = cost;
        self.came_from[idx] = parent_idx;
    }

    #[inline]
    fn parent(&self, idx: usize) -> u32 {
        self.came_from[idx]
    }
}

thread_local! {
    static GRID_NAV_SCRATCH: RefCell<GridNavScratch> = RefCell::new(GridNavScratch::new());
}

#[derive(Debug, Clone)]
pub struct GridNav {
    width: i32,
    height: i32,
    blocked: Vec<bool>,
    cell_size: f32,
    origin_x: f32,
    origin_y: f32,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct Node {
    pos: (i32, i32),
    cost: i32,
    estimate: i32,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.cost + other.estimate).cmp(&(self.cost + self.estimate))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl GridNav {
    pub fn new(width: i32, height: i32, cell_size: f32) -> Self {
        Self::with_origin(width, height, cell_size, 0.0, 0.0)
    }

    pub fn with_origin(
        width: i32,
        height: i32,
        cell_size: f32,
        origin_x: f32,
        origin_y: f32,
    ) -> Self {
        Self {
            width,
            height,
            blocked: vec![false; (width * height).max(0) as usize],
            cell_size,
            origin_x,
            origin_y,
        }
    }

    pub fn set_blocked(&mut self, x: i32, y: i32, blocked: bool) {
        if let Some(idx) = self.index(x, y) {
            self.blocked[idx] = blocked;
        }
    }

    pub fn world_to_grid(&self, world_x: f32, world_y: f32) -> Option<(i32, i32)> {
        if !world_x.is_finite() || !world_y.is_finite() {
            return None;
        }
        let grid_x = ((world_x - self.origin_x) / self.cell_size).floor() as i32;
        let grid_y = ((world_y - self.origin_y) / self.cell_size).floor() as i32;
        if self.is_in_bounds(grid_x, grid_y) {
            Some((grid_x, grid_y))
        } else {
            None
        }
    }

    pub fn find_path_world(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
        let from_grid = self.world_to_grid(from.x, from.y)?;
        let to_grid = self.world_to_grid(to.x, to.y)?;
        self.find_path(from_grid, to_grid)
    }

    /// A* pathfinding using pre-allocated flat buffers (thread-local) instead of
    /// per-call HashMap allocations.  The scratch buffers use a generation counter
    /// so resetting is O(1).
    pub fn find_path(&self, from: (i32, i32), to: (i32, i32)) -> Option<Vec<Vec2>> {
        if !self.is_in_bounds(from.0, from.1) || !self.is_in_bounds(to.0, to.1) {
            return None;
        }
        if self.is_blocked(from.0, from.1) || self.is_blocked(to.0, to.1) {
            return None;
        }
        if from == to {
            return Some(vec![self.grid_to_world(from.0, from.1)]);
        }

        let node_count = (self.width * self.height) as usize;
        let width = self.width;

        GRID_NAV_SCRATCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            scratch.reset(node_count);

            let mut frontier = BinaryHeap::new();
            let from_idx = (from.1 * width + from.0) as usize;
            scratch.set(from_idx, 0, from_idx as u32);

            frontier.push(Node {
                pos: from,
                cost: 0,
                estimate: octile_heuristic(from, to),
            });

            while let Some(current) = frontier.pop() {
                if current.pos == to {
                    return Some(self.reconstruct_path_flat(from, to, &scratch));
                }

                let current_idx = (current.pos.1 * width + current.pos.0) as usize;
                // Skip if we already found a better path to this node
                // (duplicate entries in the frontier with higher cost).
                if current.cost > scratch.get_cost(current_idx) {
                    continue;
                }

                for (next, step_cost) in neighbors(current.pos) {
                    if step_cost == COST_DIAGONAL {
                        let dx = next.0 - current.pos.0;
                        let dy = next.1 - current.pos.1;
                        // Prevent diagonal corner cutting through blocked orthogonal neighbors.
                        if self.is_blocked(current.pos.0 + dx, current.pos.1)
                            || self.is_blocked(current.pos.0, current.pos.1 + dy)
                        {
                            continue;
                        }
                    }
                    if !self.is_in_bounds(next.0, next.1) || self.is_blocked(next.0, next.1) {
                        continue;
                    }
                    let next_idx = (next.1 * width + next.0) as usize;
                    let new_cost = current.cost + step_cost;
                    let old = scratch.get_cost(next_idx);
                    if new_cost < old {
                        scratch.set(next_idx, new_cost, current_idx as u32);
                        frontier.push(Node {
                            pos: next,
                            cost: new_cost,
                            estimate: octile_heuristic(next, to),
                        });
                    }
                }
            }

            None
        })
    }

    /// Reconstruct path from flat scratch buffers.
    fn reconstruct_path_flat(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        scratch: &GridNavScratch,
    ) -> Vec<Vec2> {
        let width = self.width;
        let from_idx = (from.1 * width + from.0) as u32;
        let mut current_idx = (to.1 * width + to.0) as u32;
        let mut path_indices = vec![current_idx];

        while current_idx != from_idx {
            let parent = scratch.parent(current_idx as usize);
            if parent == current_idx {
                break; // safety: prevent infinite loop
            }
            current_idx = parent;
            path_indices.push(current_idx);
        }

        path_indices.reverse();
        path_indices
            .into_iter()
            .map(|idx| {
                let x = (idx as i32) % width;
                let y = (idx as i32) / width;
                self.grid_to_world(x, y)
            })
            .collect()
    }

    fn grid_to_world(&self, x: i32, y: i32) -> Vec2 {
        Vec2::new(
            self.origin_x + (x as f32 + 0.5) * self.cell_size,
            self.origin_y + (y as f32 + 0.5) * self.cell_size,
        )
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        Some((y * self.width + x) as usize)
    }

    fn is_in_bounds(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some()
    }

    fn is_blocked(&self, x: i32, y: i32) -> bool {
        self.index(x, y)
            .map(|idx| self.blocked[idx])
            .unwrap_or(true)
    }
}

const COST_CARDINAL: i32 = 10;
const COST_DIAGONAL: i32 = 14;

fn neighbors(pos: (i32, i32)) -> [((i32, i32), i32); 8] {
    [
        ((pos.0 + 1, pos.1), COST_CARDINAL),
        ((pos.0 - 1, pos.1), COST_CARDINAL),
        ((pos.0, pos.1 + 1), COST_CARDINAL),
        ((pos.0, pos.1 - 1), COST_CARDINAL),
        ((pos.0 + 1, pos.1 + 1), COST_DIAGONAL),
        ((pos.0 - 1, pos.1 + 1), COST_DIAGONAL),
        ((pos.0 + 1, pos.1 - 1), COST_DIAGONAL),
        ((pos.0 - 1, pos.1 - 1), COST_DIAGONAL),
    ]
}

fn octile_heuristic(a: (i32, i32), b: (i32, i32)) -> i32 {
    let dx = (a.0 - b.0).abs();
    let dy = (a.1 - b.1).abs();
    let diagonal_steps = dx.min(dy);
    let straight_steps = dx.max(dy) - diagonal_steps;
    (diagonal_steps * COST_DIAGONAL) + (straight_steps * COST_CARDINAL)
}

#[derive(Debug, Clone)]
pub struct NavPolygon {
    pub vertices: Vec<Vec2>,
    pub centroid: Vec2,
    pub neighbors: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct NavMesh {
    polygons: Vec<NavPolygon>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct PolyNode {
    poly_idx: usize,
    cost: i32,
    estimate: i32,
}

impl Ord for PolyNode {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.cost + other.estimate).cmp(&(self.cost + self.estimate))
    }
}

impl PartialOrd for PolyNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Thread-local scratch buffers for `NavMesh::find_polygon_path`, using the
/// same generation-counter technique as `GridNavScratch`.
struct PolyNavScratch {
    generation: u32,
    visited_gen: Vec<u32>,
    cost_so_far: Vec<i32>,
    came_from: Vec<u32>,
}

impl PolyNavScratch {
    fn new() -> Self {
        Self {
            generation: 0,
            visited_gen: Vec::new(),
            cost_so_far: Vec::new(),
            came_from: Vec::new(),
        }
    }

    fn reset(&mut self, node_count: usize) {
        if self.generation == u32::MAX {
            self.visited_gen.clear();
            self.generation = 0;
        }
        self.generation += 1;
        if self.visited_gen.len() < node_count {
            self.visited_gen.resize(node_count, 0);
            self.cost_so_far.resize(node_count, 0);
            self.came_from.resize(node_count, 0);
        }
    }

    #[inline]
    fn is_visited(&self, idx: usize) -> bool {
        self.visited_gen.get(idx).copied() == Some(self.generation)
    }

    #[inline]
    fn get_cost(&self, idx: usize) -> i32 {
        if self.is_visited(idx) {
            self.cost_so_far[idx]
        } else {
            i32::MAX
        }
    }

    #[inline]
    fn set(&mut self, idx: usize, cost: i32, parent_idx: u32) {
        self.visited_gen[idx] = self.generation;
        self.cost_so_far[idx] = cost;
        self.came_from[idx] = parent_idx;
    }

    #[inline]
    fn parent(&self, idx: usize) -> u32 {
        self.came_from[idx]
    }
}

thread_local! {
    static POLY_NAV_SCRATCH: RefCell<PolyNavScratch> = RefCell::new(PolyNavScratch::new());
}

impl NavMesh {
    pub fn from_convex_polygons(polygons: Vec<Vec<Vec2>>) -> Self {
        let mut nav_polys: Vec<NavPolygon> = polygons
            .into_iter()
            .filter(|poly| poly.len() >= 3)
            .map(|vertices| {
                let centroid = centroid(&vertices);
                NavPolygon {
                    vertices,
                    centroid,
                    neighbors: Vec::new(),
                }
            })
            .collect();

        let count = nav_polys.len();
        for i in 0..count {
            for j in (i + 1)..count {
                if polygons_share_edge(&nav_polys[i].vertices, &nav_polys[j].vertices) {
                    nav_polys[i].neighbors.push(j);
                    nav_polys[j].neighbors.push(i);
                }
            }
        }

        Self {
            polygons: nav_polys,
        }
    }

    pub fn polygon_count(&self) -> usize {
        self.polygons.len()
    }

    pub fn find_path(&self, start: Vec2, end: Vec2) -> Option<Vec<Vec2>> {
        if self.polygons.is_empty() {
            return None;
        }

        let start_poly = self
            .find_polygon_containing(start)
            .or_else(|| self.closest_polygon(start))?;
        let end_poly = self
            .find_polygon_containing(end)
            .or_else(|| self.closest_polygon(end))?;

        if start_poly == end_poly {
            return Some(vec![start, end]);
        }

        let poly_path = self.find_polygon_path(start_poly, end_poly)?;
        let mut waypoints = Vec::with_capacity(poly_path.len() + 2);
        waypoints.push(start);
        for poly_idx in poly_path
            .iter()
            .skip(1)
            .take(poly_path.len().saturating_sub(2))
        {
            waypoints.push(self.polygons[*poly_idx].centroid);
        }
        waypoints.push(end);
        Some(waypoints)
    }

    fn find_polygon_containing(&self, p: Vec2) -> Option<usize> {
        self.polygons.iter().enumerate().find_map(|(idx, poly)| {
            if point_in_polygon(p, &poly.vertices) {
                Some(idx)
            } else {
                None
            }
        })
    }

    fn closest_polygon(&self, p: Vec2) -> Option<usize> {
        self.polygons
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = dist_sq(a.centroid, p);
                let db = dist_sq(b.centroid, p);
                da.partial_cmp(&db).unwrap_or(Ordering::Equal)
            })
            .map(|(idx, _)| idx)
    }

    /// A* over the polygon adjacency graph, using pre-allocated flat scratch
    /// buffers (thread-local) indexed by polygon index.
    fn find_polygon_path(&self, start_poly: usize, end_poly: usize) -> Option<Vec<usize>> {
        let poly_count = self.polygons.len();

        POLY_NAV_SCRATCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            scratch.reset(poly_count);

            let mut frontier = BinaryHeap::new();
            scratch.set(start_poly, 0, start_poly as u32);

            frontier.push(PolyNode {
                poly_idx: start_poly,
                cost: 0,
                estimate: poly_heuristic(
                    self.polygons[start_poly].centroid,
                    self.polygons[end_poly].centroid,
                ),
            });

            while let Some(current) = frontier.pop() {
                if current.poly_idx == end_poly {
                    return Some(reconstruct_poly_path_flat(start_poly, end_poly, &scratch));
                }

                if current.cost > scratch.get_cost(current.poly_idx) {
                    continue;
                }

                for neighbor in &self.polygons[current.poly_idx].neighbors {
                    let new_cost = current.cost + 1;
                    let old = scratch.get_cost(*neighbor);
                    if new_cost < old {
                        scratch.set(*neighbor, new_cost, current.poly_idx as u32);
                        frontier.push(PolyNode {
                            poly_idx: *neighbor,
                            cost: new_cost,
                            estimate: poly_heuristic(
                                self.polygons[*neighbor].centroid,
                                self.polygons[end_poly].centroid,
                            ),
                        });
                    }
                }
            }

            None
        })
    }
}

fn reconstruct_poly_path_flat(start: usize, end: usize, scratch: &PolyNavScratch) -> Vec<usize> {
    let mut current = end as u32;
    let start_u32 = start as u32;
    let mut path = vec![end];
    while current != start_u32 {
        let parent = scratch.parent(current as usize);
        if parent == current {
            break; // safety: prevent infinite loop
        }
        current = parent;
        path.push(current as usize);
    }
    path.reverse();
    path
}

fn poly_heuristic(a: Vec2, b: Vec2) -> i32 {
    (a.x - b.x).abs().round() as i32 + (a.y - b.y).abs().round() as i32
}

fn centroid(vertices: &[Vec2]) -> Vec2 {
    let mut sx = 0.0;
    let mut sy = 0.0;
    for v in vertices {
        sx += v.x;
        sy += v.y;
    }
    let n = vertices.len().max(1) as f32;
    Vec2::new(sx / n, sy / n)
}

fn dist_sq(a: Vec2, b: Vec2) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

fn points_close(a: Vec2, b: Vec2) -> bool {
    const EPS: f32 = 0.001;
    (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS
}

fn polygons_share_edge(a: &[Vec2], b: &[Vec2]) -> bool {
    let mut shared = 0usize;
    for av in a {
        for bv in b {
            if points_close(*av, *bv) {
                shared += 1;
                if shared >= 2 {
                    return true;
                }
            }
        }
    }
    false
}

fn point_in_polygon(point: Vec2, vertices: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = vertices.len().saturating_sub(1);

    for i in 0..vertices.len() {
        let vi = vertices[i];
        let vj = vertices[j];
        let intersect = ((vi.y > point.y) != (vj.y > point.y))
            && (point.x
                < (vj.x - vi.x) * (point.y - vi.y) / ((vj.y - vi.y).abs().max(0.00001)) + vi.x);
        if intersect {
            inside = !inside;
        }
        j = i;
    }

    inside
}

#[cfg(test)]
mod tests {
    use super::{GridNav, NavMesh};
    use crate::core::types::Vec2;

    #[test]
    fn grid_nav_finds_path() {
        let mut nav = GridNav::new(8, 8, 1.0);
        nav.set_blocked(3, 3, true);
        nav.set_blocked(3, 4, true);
        let path = nav.find_path((1, 1), (6, 6));
        assert!(path.is_some());
    }

    #[test]
    fn grid_nav_uses_diagonal_steps_when_clear() {
        let nav = GridNav::new(8, 8, 1.0);
        let path = nav.find_path((1, 1), (6, 6)).expect("path should exist");
        // 8-direction A* should produce fewer than pure 4-direction steps here.
        assert!(path.len() < 11);
    }

    #[test]
    fn grid_nav_blocks_diagonal_corner_cutting() {
        let mut nav = GridNav::new(4, 4, 1.0);
        nav.set_blocked(1, 0, true);
        nav.set_blocked(0, 1, true);
        let path = nav.find_path((0, 0), (1, 1));
        assert!(path.is_none());
    }

    #[test]
    fn grid_nav_returns_none_when_start_blocked() {
        let mut nav = GridNav::new(4, 4, 1.0);
        nav.set_blocked(0, 0, true);
        let path = nav.find_path((0, 0), (3, 3));
        assert!(path.is_none());
    }

    #[test]
    fn grid_nav_returns_none_when_out_of_bounds() {
        let nav = GridNav::new(4, 4, 1.0);
        assert!(nav.find_path((-1, 0), (3, 3)).is_none());
        assert!(nav.find_path((0, 0), (4, 3)).is_none());
    }

    #[test]
    fn grid_nav_returns_single_waypoint_when_already_at_target() {
        let nav = GridNav::new(4, 4, 10.0);
        let path = nav.find_path((2, 1), (2, 1)).expect("path should exist");
        assert_eq!(path.len(), 1);
        assert!((path[0].x - 25.0).abs() < 0.001);
        assert!((path[0].y - 15.0).abs() < 0.001);
    }

    #[test]
    fn grid_nav_scratch_reuse_across_calls() {
        // Verify that multiple pathfinding calls on the same thread produce
        // correct results (i.e. the generation counter properly resets state).
        let nav = GridNav::new(8, 8, 1.0);
        for _ in 0..10 {
            let path = nav.find_path((0, 0), (7, 7));
            assert!(path.is_some());
            let waypoints = path.unwrap();
            let last = waypoints.last().unwrap();
            assert!((last.x - 7.5).abs() < 0.001);
            assert!((last.y - 7.5).abs() < 0.001);
        }
    }

    #[test]
    fn navmesh_path_across_adjacent_polygons() {
        let mesh = NavMesh::from_convex_polygons(vec![
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(0.0, 10.0),
            ],
            vec![
                Vec2::new(10.0, 0.0),
                Vec2::new(20.0, 0.0),
                Vec2::new(20.0, 10.0),
                Vec2::new(10.0, 10.0),
            ],
        ]);

        assert_eq!(mesh.polygon_count(), 2);
        let path = mesh.find_path(Vec2::new(2.0, 2.0), Vec2::new(18.0, 2.0));
        assert!(path.is_some());
        let points = path.expect("path should exist");
        assert!(points.len() >= 2);
        assert!((points.first().unwrap().x - 2.0).abs() < 0.001);
        assert!((points.last().unwrap().x - 18.0).abs() < 0.001);
    }
}
