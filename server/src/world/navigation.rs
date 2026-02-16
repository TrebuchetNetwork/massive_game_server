// massive_game_server/server/src/world/navigation.rs

use crate::core::types::Vec2;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Debug, Clone)]
pub struct GridNav {
    width: i32,
    height: i32,
    blocked: Vec<bool>,
    cell_size: f32,
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
        Self {
            width,
            height,
            blocked: vec![false; (width * height).max(0) as usize],
            cell_size,
        }
    }

    pub fn set_blocked(&mut self, x: i32, y: i32, blocked: bool) {
        if let Some(idx) = self.index(x, y) {
            self.blocked[idx] = blocked;
        }
    }

    pub fn find_path(&self, from: (i32, i32), to: (i32, i32)) -> Option<Vec<Vec2>> {
        if self.is_blocked(to.0, to.1) {
            return None;
        }

        let mut frontier = BinaryHeap::new();
        let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
        let mut cost_so_far: HashMap<(i32, i32), i32> = HashMap::new();

        frontier.push(Node {
            pos: from,
            cost: 0,
            estimate: manhattan(from, to),
        });
        cost_so_far.insert(from, 0);

        while let Some(current) = frontier.pop() {
            if current.pos == to {
                return Some(self.reconstruct_path(from, to, &came_from));
            }

            for next in neighbors(current.pos) {
                if self.is_blocked(next.0, next.1) {
                    continue;
                }
                let new_cost = current.cost + 1;
                let old = cost_so_far.get(&next).copied().unwrap_or(i32::MAX);
                if new_cost < old {
                    cost_so_far.insert(next, new_cost);
                    came_from.insert(next, current.pos);
                    frontier.push(Node {
                        pos: next,
                        cost: new_cost,
                        estimate: manhattan(next, to),
                    });
                }
            }
        }

        None
    }

    fn reconstruct_path(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        came_from: &HashMap<(i32, i32), (i32, i32)>,
    ) -> Vec<Vec2> {
        let mut current = to;
        let mut path = vec![to];
        while current != from {
            let Some(prev) = came_from.get(&current).copied() else {
                break;
            };
            current = prev;
            path.push(current);
        }
        path.reverse();
        path.into_iter().map(|p| self.grid_to_world(p.0, p.1)).collect()
    }

    fn grid_to_world(&self, x: i32, y: i32) -> Vec2 {
        Vec2::new(
            (x as f32 + 0.5) * self.cell_size,
            (y as f32 + 0.5) * self.cell_size,
        )
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        Some((y * self.width + x) as usize)
    }

    fn is_blocked(&self, x: i32, y: i32) -> bool {
        self.index(x, y)
            .map(|idx| self.blocked[idx])
            .unwrap_or(true)
    }
}

fn neighbors(pos: (i32, i32)) -> [(i32, i32); 4] {
    [
        (pos.0 + 1, pos.1),
        (pos.0 - 1, pos.1),
        (pos.0, pos.1 + 1),
        (pos.0, pos.1 - 1),
    ]
}

fn manhattan(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}
