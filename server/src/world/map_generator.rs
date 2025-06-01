// massive_game_server/server/src/world/map_generator.rs
use crate::core::types::{Wall, Vec2}; // Removed unused EntityId
use crate::core::constants::*;
use uuid::Uuid;
use rand::Rng;

pub struct MapGenerator;

impl MapGenerator {
    pub fn generate_10v10_map() -> Vec<Wall> {
        let mut walls = Vec::new();
        let mut rng = rand::thread_rng();

        walls.extend(Self::create_border_walls());
        walls.extend(Self::create_central_arena_open());
        walls.extend(Self::create_team_bases_open());
        walls.extend(Self::create_strategic_cover_sparse(&mut rng));
        walls.extend(Self::create_destructible_nodes_sparse(&mut rng));
        walls.extend(Self::create_lanes_and_pathways(&mut rng)); // rng is used here
        
        walls
    }

    fn create_border_walls() -> Vec<Wall> {
        let mut walls = Vec::new();
        let thickness = 20.0; 

        walls.push(Wall {
            id: Uuid::new_v4().as_u128() as u64, x: WORLD_MIN_X, y: WORLD_MIN_Y,
            width: WORLD_MAX_X - WORLD_MIN_X, height: thickness,
            is_destructible: false, current_health: 1000, max_health: 1000,
        });
        walls.push(Wall {
            id: Uuid::new_v4().as_u128() as u64, x: WORLD_MIN_X, y: WORLD_MAX_Y - thickness,
            width: WORLD_MAX_X - WORLD_MIN_X, height: thickness,
            is_destructible: false, current_health: 1000, max_health: 1000,
        });
        walls.push(Wall {
            id: Uuid::new_v4().as_u128() as u64, x: WORLD_MIN_X, y: WORLD_MIN_Y,
            width: thickness, height: WORLD_MAX_Y - WORLD_MIN_Y,
            is_destructible: false, current_health: 1000, max_health: 1000,
        });
        walls.push(Wall {
            id: Uuid::new_v4().as_u128() as u64, x: WORLD_MAX_X - thickness, y: WORLD_MIN_Y,
            width: thickness, height: WORLD_MAX_Y - WORLD_MIN_Y,
            is_destructible: false, current_health: 1000, max_health: 1000,
        });
        walls
    }

    fn create_central_arena_open() -> Vec<Wall> {
        let mut walls = Vec::new();
        let center_x = 0.0;
        let center_y = 0.0;
        let arena_radius = 200.0; 
        let _opening_size = 150.0; // Prefixed with underscore as it's unused
        let wall_thickness = 15.0;

        let pillar_size = 80.0;
        let offset = arena_radius - pillar_size / 2.0;

        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x - offset - pillar_size/2.0, y: center_y - offset - pillar_size/2.0, width: pillar_size, height: wall_thickness, is_destructible: false, current_health: 500, max_health: 500 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x - offset - pillar_size/2.0, y: center_y - offset - pillar_size/2.0, width: wall_thickness, height: pillar_size, is_destructible: false, current_health: 500, max_health: 500 });

        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x + offset - pillar_size/2.0, y: center_y - offset - pillar_size/2.0, width: pillar_size, height: wall_thickness, is_destructible: false, current_health: 500, max_health: 500 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x + offset + pillar_size/2.0 - wall_thickness, y: center_y - offset - pillar_size/2.0, width: wall_thickness, height: pillar_size, is_destructible: false, current_health: 500, max_health: 500 });
        
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x - offset - pillar_size/2.0, y: center_y + offset + pillar_size/2.0 - wall_thickness, width: pillar_size, height: wall_thickness, is_destructible: false, current_health: 500, max_health: 500 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x - offset - pillar_size/2.0, y: center_y + offset - pillar_size/2.0, width: wall_thickness, height: pillar_size, is_destructible: false, current_health: 500, max_health: 500 });

        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x + offset - pillar_size/2.0, y: center_y + offset + pillar_size/2.0 - wall_thickness, width: pillar_size, height: wall_thickness, is_destructible: false, current_health: 500, max_health: 500 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: center_x + offset + pillar_size/2.0 - wall_thickness, y: center_y + offset - pillar_size/2.0, width: wall_thickness, height: pillar_size, is_destructible: false, current_health: 500, max_health: 500 });

        walls
    }

    fn create_team_bases_open() -> Vec<Wall> {
        let mut walls = Vec::new();
        let base_depth = 250.0;
        let base_width = 400.0;
        let wall_thickness = 20.0;

        let t1_base_x = WORLD_MIN_X + wall_thickness;
        let t1_base_y_center = 0.0;
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: t1_base_x, y: t1_base_y_center - base_width/2.0, width: wall_thickness, height: base_width, is_destructible: false, current_health: 1000, max_health: 1000 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: t1_base_x, y: t1_base_y_center - base_width/2.0, width: base_depth * 0.6, height: wall_thickness, is_destructible: false, current_health: 700, max_health: 700 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: t1_base_x, y: t1_base_y_center + base_width/2.0 - wall_thickness, width: base_depth * 0.6, height: wall_thickness, is_destructible: false, current_health: 700, max_health: 700 });
        walls.push(Wall {id: Uuid::new_v4().as_u128() as u64, x: t1_base_x + base_depth * 0.3, y: t1_base_y_center - 50.0, width: 60.0, height: 25.0, is_destructible: true, current_health: 150, max_health: 150 });

        let t2_base_x = WORLD_MAX_X - base_depth - wall_thickness;
        let t2_base_y_center = 0.0;
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: WORLD_MAX_X - wall_thickness * 2.0, y: t2_base_y_center - base_width/2.0, width: wall_thickness, height: base_width, is_destructible: false, current_health: 1000, max_health: 1000 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: t2_base_x + base_depth * 0.4 - wall_thickness, y: t2_base_y_center - base_width/2.0, width: base_depth * 0.6, height: wall_thickness, is_destructible: false, current_health: 700, max_health: 700 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: t2_base_x + base_depth * 0.4 - wall_thickness, y: t2_base_y_center + base_width/2.0 - wall_thickness, width: base_depth * 0.6, height: wall_thickness, is_destructible: false, current_health: 700, max_health: 700 });
        walls.push(Wall {id: Uuid::new_v4().as_u128() as u64, x: t2_base_x + base_depth * 0.7 - 60.0 , y: t2_base_y_center + 50.0, width: 60.0, height: 25.0, is_destructible: true, current_health: 150, max_health: 150 });

        walls
    }

    fn create_strategic_cover_sparse(rng: &mut impl Rng) -> Vec<Wall> {
        let mut walls = Vec::new();
        let number_of_cover_points = 6; 
        let cover_health = 120;

        for _ in 0..number_of_cover_points {
            let x = rng.gen_range(WORLD_MIN_X + 200.0 .. WORLD_MAX_X - 200.0);
            let y = rng.gen_range(WORLD_MIN_Y + 200.0 .. WORLD_MAX_Y - 200.0);
            
            if x.abs() < 250.0 && y.abs() < 250.0 { continue; } 
            if x < WORLD_MIN_X + 400.0 || x > WORLD_MAX_X - 400.0 { continue; } 

            let width = rng.gen_range(40.0..80.0);
            let height = rng.gen_range(15.0..30.0); 
            walls.push(Wall {
                id: Uuid::new_v4().as_u128() as u64, x, y, width, height,
                is_destructible: true, current_health: cover_health, max_health: cover_health,
            });
        }
        walls
    }

    fn create_destructible_nodes_sparse(rng: &mut impl Rng) -> Vec<Wall> {
        let mut walls = Vec::new();
        let number_of_nodes = 3; 
        let node_health = 200;

        for _ in 0..number_of_nodes {
            let x = rng.gen_range(WORLD_MIN_X + 300.0 .. WORLD_MAX_X - 300.0);
            let y = rng.gen_range(WORLD_MIN_Y + 300.0 .. WORLD_MAX_Y - 300.0);

            if x.abs() < 150.0 && y.abs() < 150.0 { continue; }

            let size = rng.gen_range(50.0..70.0); 
            walls.push(Wall {
                id: Uuid::new_v4().as_u128() as u64, x, y, width: size, height: size,
                is_destructible: true, current_health: node_health, max_health: node_health,
            });
        }
        walls
    }

    fn create_lanes_and_pathways(_rng: &mut impl Rng) -> Vec<Wall> { // Prefixed rng as it's not used in this version
        let mut walls = Vec::new();
        let wall_thickness = 15.0;
        let lane_wall_health = 300;

        let top_y_divider = WORLD_MIN_Y / 3.0;
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: WORLD_MIN_X + 300.0, y: top_y_divider, width: 400.0, height: wall_thickness, is_destructible: true, current_health: lane_wall_health, max_health: lane_wall_health });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: WORLD_MAX_X - 700.0, y: top_y_divider, width: 400.0, height: wall_thickness, is_destructible: true, current_health: lane_wall_health, max_health: lane_wall_health });
        
        let bottom_y_divider = WORLD_MAX_Y / 3.0;
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: WORLD_MIN_X + 300.0, y: bottom_y_divider, width: 400.0, height: wall_thickness, is_destructible: true, current_health: lane_wall_health, max_health: lane_wall_health });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: WORLD_MAX_X - 700.0, y: bottom_y_divider, width: 400.0, height: wall_thickness, is_destructible: true, current_health: lane_wall_health, max_health: lane_wall_health });

        let mid_x1 = -200.0;
        let mid_x2 = 200.0;
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: mid_x1, y: WORLD_MIN_Y + 100.0, width: wall_thickness, height: 150.0, is_destructible: false, current_health: 500, max_health: 500 });
        walls.push(Wall { id: Uuid::new_v4().as_u128() as u64, x: mid_x2, y: WORLD_MAX_Y - 250.0, width: wall_thickness, height: 150.0, is_destructible: false, current_health: 500, max_health: 500 });

        walls
    }
    
    pub fn get_team_spawn_areas() -> Vec<(Vec2, u8)> { 
        let mut spawns = Vec::new();
        let base_depth = 250.0;
        let base_width_half = 200.0; 

        let t1_center_x = WORLD_MIN_X + base_depth * 0.5;
        let t1_center_y = 0.0;
        spawns.push((Vec2::new(t1_center_x, t1_center_y - base_width_half * 0.5), 1));
        spawns.push((Vec2::new(t1_center_x, t1_center_y + base_width_half * 0.5), 1));
        spawns.push((Vec2::new(t1_center_x + 50.0, t1_center_y), 1));
        spawns.push((Vec2::new(t1_center_x - 50.0, t1_center_y - base_width_half * 0.25), 1));
        spawns.push((Vec2::new(t1_center_x - 50.0, t1_center_y + base_width_half * 0.25), 1));


        let t2_center_x = WORLD_MAX_X - base_depth * 0.5;
        let t2_center_y = 0.0;
        spawns.push((Vec2::new(t2_center_x, t2_center_y - base_width_half * 0.5), 2));
        spawns.push((Vec2::new(t2_center_x, t2_center_y + base_width_half * 0.5), 2));
        spawns.push((Vec2::new(t2_center_x - 50.0, t2_center_y), 2));
        spawns.push((Vec2::new(t2_center_x + 50.0, t2_center_y - base_width_half * 0.25), 2));
        spawns.push((Vec2::new(t2_center_x + 50.0, t2_center_y + base_width_half * 0.25), 2));
        
        spawns.push((Vec2::new(WORLD_MIN_X + 100.0, WORLD_MAX_Y - 100.0), 0)); 
        spawns.push((Vec2::new(WORLD_MAX_X - 100.0, WORLD_MIN_Y + 100.0), 0)); 

        spawns
    }
}
