use crate::core::types::Wall;
use parking_lot::RwLock;
use rstar::{RTree, RTreeObject, SelectionFunction, AABB};
use std::sync::Arc;

struct SelectById {
    id: crate::core::types::EntityId,
}

impl SelectionFunction<SpatialWall> for SelectById {
    fn should_unpack_parent(&self, _envelope: &<SpatialWall as RTreeObject>::Envelope) -> bool {
        true
    }

    fn should_unpack_leaf(&self, leaf: &SpatialWall) -> bool {
        leaf.wall.id == self.id
    }
}
use tracing::debug;

#[derive(Clone, Debug)]
struct SpatialWall {
    wall: Wall,
}

impl RTreeObject for SpatialWall {
    type Envelope = AABB<[f32; 2]>;

    fn envelope(&self) -> Self::Envelope {
        let min = [self.wall.x, self.wall.y];
        let max = [
            self.wall.x + self.wall.width,
            self.wall.y + self.wall.height,
        ];
        AABB::from_corners(min, max)
    }
}

pub struct WallSpatialIndex {
    rtree: Arc<RwLock<RTree<SpatialWall>>>,
    last_update_frame: Arc<RwLock<u64>>,
}

impl Default for WallSpatialIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl WallSpatialIndex {
    pub fn new() -> Self {
        WallSpatialIndex {
            rtree: Arc::new(RwLock::new(RTree::new())),
            last_update_frame: Arc::new(RwLock::new(0)),
        }
    }

    /// Build or rebuild the spatial index from a collection of walls
    pub fn rebuild(&self, walls: &[Wall], frame: u64) {
        let spatial_walls: Vec<SpatialWall> = walls
            .iter()
            .filter(|w| !w.is_destructible || w.current_health > 0)
            .map(|w| SpatialWall { wall: w.clone() })
            .collect();

        let new_tree = RTree::bulk_load(spatial_walls);

        let mut tree_guard = self.rtree.write();
        *tree_guard = new_tree;

        let mut frame_guard = self.last_update_frame.write();
        *frame_guard = frame;

        debug!(
            "Wall spatial index rebuilt at frame {} with {} walls",
            frame,
            tree_guard.size()
        );
    }

    pub fn update_walls(
        &self,
        removed_ids: &[crate::core::types::EntityId],
        added_walls: &[Wall],
        frame: u64,
    ) {
        if removed_ids.is_empty() && added_walls.is_empty() {
            return;
        }

        let mut tree_guard = self.rtree.write();

        for id in removed_ids {
            tree_guard.remove_with_selection_function(SelectById { id: *id });
        }

        for wall in added_walls {
            if !wall.is_destructible || wall.current_health > 0 {
                tree_guard.insert(SpatialWall { wall: wall.clone() });
            }
        }

        let mut frame_guard = self.last_update_frame.write();
        *frame_guard = frame;
    }

    fn for_each_in_aabb<F>(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32, mut visit: F)
    where
        F: FnMut(&Wall),
    {
        let query_aabb = AABB::from_corners([min_x, min_y], [max_x, max_y]);

        let tree_guard = self.rtree.read();
        for spatial_wall in tree_guard.locate_in_envelope_intersecting(&query_aabb) {
            visit(&spatial_wall.wall);
        }
    }

    /// Query walls that intersect with a given AABB
    pub fn query_aabb(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Vec<Wall> {
        let mut walls = Vec::new();
        self.for_each_in_aabb(min_x, min_y, max_x, max_y, |wall| {
            walls.push(wall.clone());
        });
        walls
    }

    /// Query walls within a radius of a point
    pub fn query_radius(&self, x: f32, y: f32, radius: f32) -> Vec<Wall> {
        self.query_aabb(x - radius, y - radius, x + radius, y + radius)
    }

    /// Query walls along a line segment (for projectile paths)
    pub fn query_line_segment(&self, x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<Wall> {
        let mut walls = Vec::new();
        self.for_each_line_segment_candidate(x1, y1, x2, y2, |wall| {
            walls.push(wall.clone());
        });
        walls
    }

    /// Visit walls whose AABB intersects the line segment bounds. This avoids
    /// allocating a candidate Vec on hot collision paths.
    pub fn for_each_line_segment_candidate<F>(&self, x1: f32, y1: f32, x2: f32, y2: f32, visit: F)
    where
        F: FnMut(&Wall),
    {
        // Get bounding box of the line segment
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);

        // Add a small buffer for edge cases
        let buffer = 1.0;
        self.for_each_in_aabb(
            min_x - buffer,
            min_y - buffer,
            max_x + buffer,
            max_y + buffer,
            visit,
        )
    }

    /// Get the frame number when the index was last updated
    pub fn last_update_frame(&self) -> u64 {
        *self.last_update_frame.read()
    }

    /// Check if the index needs rebuilding based on frame number
    pub fn needs_rebuild(&self, current_frame: u64, rebuild_interval: u64) -> bool {
        let last_frame = self.last_update_frame();
        current_frame >= last_frame + rebuild_interval
    }

    /// Get the number of walls in the index
    pub fn size(&self) -> usize {
        self.rtree.read().size()
    }

    /// Clear the spatial index
    pub fn clear(&self) {
        let mut tree_guard = self.rtree.write();
        *tree_guard = RTree::new();

        let mut frame_guard = self.last_update_frame.write();
        *frame_guard = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wall_spatial_index() {
        let index = WallSpatialIndex::new();

        let walls = vec![
            Wall {
                id: 1,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                is_destructible: false,
                current_health: 100,
                max_health: 100,
            },
            Wall {
                id: 2,
                x: 20.0,
                y: 20.0,
                width: 10.0,
                height: 10.0,
                is_destructible: false,
                current_health: 100,
                max_health: 100,
            },
        ];

        index.rebuild(&walls, 1);
        assert_eq!(index.size(), 2);

        // Query that should find wall 1
        let results = index.query_aabb(-5.0, -5.0, 5.0, 5.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 1);

        // Query that should find wall 2
        let results = index.query_radius(25.0, 25.0, 10.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 2);

        // Query that should find both walls
        let results = index.query_aabb(-5.0, -5.0, 35.0, 35.0);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn line_segment_candidate_callback_visits_matching_walls() {
        let index = WallSpatialIndex::new();

        let walls = vec![
            Wall {
                id: 1,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                is_destructible: false,
                current_health: 100,
                max_health: 100,
            },
            Wall {
                id: 2,
                x: 50.0,
                y: 50.0,
                width: 10.0,
                height: 10.0,
                is_destructible: false,
                current_health: 100,
                max_health: 100,
            },
        ];

        index.rebuild(&walls, 1);

        let mut visited = Vec::new();
        index.for_each_line_segment_candidate(-5.0, -5.0, 12.0, 12.0, |wall| {
            visited.push(wall.id);
        });
        visited.sort_unstable();

        assert_eq!(visited, vec![1]);
    }
}
