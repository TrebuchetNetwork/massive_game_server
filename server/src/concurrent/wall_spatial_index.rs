use crate::core::types::{EntityId, Wall};
use rstar::{RTree, RTreeObject, AABB};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{trace, debug};

#[derive(Clone, Debug)]
struct SpatialWall {
    wall: Wall,
}

impl RTreeObject for SpatialWall {
    type Envelope = AABB<[f32; 2]>;

    fn envelope(&self) -> Self::Envelope {
        let min = [self.wall.x, self.wall.y];
        let max = [self.wall.x + self.wall.width, self.wall.y + self.wall.height];
        AABB::from_corners(min, max)
    }
}

pub struct WallSpatialIndex {
    rtree: Arc<RwLock<RTree<SpatialWall>>>,
    last_update_frame: Arc<RwLock<u64>>,
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
        
        debug!("Wall spatial index rebuilt at frame {} with {} walls", frame, tree_guard.size());
    }

    /// Query walls that intersect with a given AABB
    pub fn query_aabb(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Vec<Wall> {
        let query_aabb = AABB::from_corners([min_x, min_y], [max_x, max_y]);
        
        let tree_guard = self.rtree.read();
        tree_guard
            .locate_in_envelope_intersecting(&query_aabb)
            .map(|spatial_wall| spatial_wall.wall.clone())
            .collect()
    }

    /// Query walls within a radius of a point
    pub fn query_radius(&self, x: f32, y: f32, radius: f32) -> Vec<Wall> {
        self.query_aabb(x - radius, y - radius, x + radius, y + radius)
    }

    /// Query walls along a line segment (for projectile paths)
    pub fn query_line_segment(&self, x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<Wall> {
        // Get bounding box of the line segment
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        
        // Add a small buffer for edge cases
        let buffer = 1.0;
        self.query_aabb(min_x - buffer, min_y - buffer, max_x + buffer, max_y + buffer)
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
}
