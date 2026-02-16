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

        Self { polygons: nav_polys }
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
        for poly_idx in poly_path.iter().skip(1).take(poly_path.len().saturating_sub(2)) {
            waypoints.push(self.polygons[*poly_idx].centroid);
        }
        waypoints.push(end);
        Some(waypoints)
    }

    fn find_polygon_containing(&self, p: Vec2) -> Option<usize> {
        self.polygons
            .iter()
            .enumerate()
            .find_map(|(idx, poly)| {
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

    fn find_polygon_path(&self, start_poly: usize, end_poly: usize) -> Option<Vec<usize>> {
        let mut frontier = BinaryHeap::new();
        let mut came_from: HashMap<usize, usize> = HashMap::new();
        let mut cost_so_far: HashMap<usize, i32> = HashMap::new();

        frontier.push(PolyNode {
            poly_idx: start_poly,
            cost: 0,
            estimate: poly_heuristic(self.polygons[start_poly].centroid, self.polygons[end_poly].centroid),
        });
        cost_so_far.insert(start_poly, 0);

        while let Some(current) = frontier.pop() {
            if current.poly_idx == end_poly {
                return Some(reconstruct_poly_path(start_poly, end_poly, &came_from));
            }

            for neighbor in &self.polygons[current.poly_idx].neighbors {
                let new_cost = current.cost + 1;
                let old = cost_so_far.get(neighbor).copied().unwrap_or(i32::MAX);
                if new_cost < old {
                    cost_so_far.insert(*neighbor, new_cost);
                    came_from.insert(*neighbor, current.poly_idx);
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
    }
}

fn reconstruct_poly_path(start: usize, end: usize, came_from: &HashMap<usize, usize>) -> Vec<usize> {
    let mut current = end;
    let mut path = vec![end];
    while current != start {
        let Some(prev) = came_from.get(&current).copied() else {
            break;
        };
        current = prev;
        path.push(current);
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
