// massive_game_server/server/src/world/map_generator.rs
use crate::core::constants::*;
use crate::core::types::{generate_entity_id, Vec2, Wall, Zone, ZoneType};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

pub struct MapGenerator;

impl MapGenerator {
    pub fn generate_10v10_map() -> Vec<Wall> {
        Self::generate_10v10_map_with_seed(10_010)
    }

    pub fn generate_10v10_map_with_seed(seed: u64) -> Vec<Wall> {
        Self::generate_map_with_density(seed, 6, 3)
    }

    pub fn generate_dynamic_map(target_players: usize) -> (Vec<Wall>, String) {
        let derived_seed = 100_000u64.wrapping_add(target_players.max(10) as u64);
        Self::generate_dynamic_map_with_seed(target_players, derived_seed)
    }

    pub fn generate_dynamic_map_with_seed(target_players: usize, seed: u64) -> (Vec<Wall>, String) {
        let target = target_players.max(10);
        let cover_points = (target / 6).clamp(6, 30);
        let destructible_nodes = (target / 14).clamp(3, 18);
        let walls = Self::generate_map_with_density(seed, cover_points, destructible_nodes);
        let map_name = format!("Massive Arena Dynamic {}p", target);
        (walls, map_name)
    }

    pub fn generate_environment_zones_with_seed(seed: u64) -> Vec<Zone> {
        let mut rng = StdRng::seed_from_u64(seed ^ 0xA5A5_5A5A_D15C_AFE5);
        let mut zones = Vec::with_capacity(6);

        zones.push(Zone {
            id: generate_entity_id(),
            x: -95.0,
            y: -320.0,
            width: 190.0,
            height: 640.0,
            zone_type: ZoneType::SlowZone,
            direction: 0.0,
        });

        zones.push(Zone {
            id: generate_entity_id(),
            x: -260.0,
            y: -70.0,
            width: 120.0,
            height: 140.0,
            zone_type: ZoneType::DamageZone,
            direction: 0.0,
        });

        zones.push(Zone {
            id: generate_entity_id(),
            x: 140.0,
            y: -70.0,
            width: 120.0,
            height: 140.0,
            zone_type: ZoneType::DamageZone,
            direction: 0.0,
        });

        let left_boost_y = rng.gen_range(-220.0..220.0);
        let right_boost_y = rng.gen_range(-220.0..220.0);
        zones.push(Zone {
            id: generate_entity_id(),
            x: WORLD_MIN_X + 120.0,
            y: left_boost_y,
            width: 80.0,
            height: 60.0,
            zone_type: ZoneType::BoostPad,
            direction: 0.0,
        });
        zones.push(Zone {
            id: generate_entity_id(),
            x: WORLD_MAX_X - 200.0,
            y: right_boost_y,
            width: 80.0,
            height: 60.0,
            zone_type: ZoneType::BoostPad,
            direction: std::f32::consts::PI,
        });

        zones
    }

    fn generate_map_with_density(
        seed: u64,
        cover_points: usize,
        destructible_nodes: usize,
    ) -> Vec<Wall> {
        let mut walls = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed);

        walls.extend(Self::create_border_walls());
        walls.extend(Self::create_central_arena_open());
        walls.extend(Self::create_team_bases_open());
        walls.extend(Self::create_strategic_cover_sparse(&mut rng, cover_points));
        walls.extend(Self::create_destructible_nodes_sparse(
            &mut rng,
            destructible_nodes,
        ));
        walls.extend(Self::create_lanes_and_pathways(&mut rng)); // rng is used here

        walls
    }

    fn create_border_walls() -> Vec<Wall> {
        let mut walls = Vec::new();
        let thickness = 20.0;

        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MIN_X,
            y: WORLD_MIN_Y,
            width: WORLD_MAX_X - WORLD_MIN_X,
            height: thickness,
            is_destructible: false,
            current_health: 1000,
            max_health: 1000,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MIN_X,
            y: WORLD_MAX_Y - thickness,
            width: WORLD_MAX_X - WORLD_MIN_X,
            height: thickness,
            is_destructible: false,
            current_health: 1000,
            max_health: 1000,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MIN_X,
            y: WORLD_MIN_Y,
            width: thickness,
            height: WORLD_MAX_Y - WORLD_MIN_Y,
            is_destructible: false,
            current_health: 1000,
            max_health: 1000,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MAX_X - thickness,
            y: WORLD_MIN_Y,
            width: thickness,
            height: WORLD_MAX_Y - WORLD_MIN_Y,
            is_destructible: false,
            current_health: 1000,
            max_health: 1000,
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

        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x - offset - pillar_size / 2.0,
            y: center_y - offset - pillar_size / 2.0,
            width: pillar_size,
            height: wall_thickness,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x - offset - pillar_size / 2.0,
            y: center_y - offset - pillar_size / 2.0,
            width: wall_thickness,
            height: pillar_size,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });

        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x + offset - pillar_size / 2.0,
            y: center_y - offset - pillar_size / 2.0,
            width: pillar_size,
            height: wall_thickness,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x + offset + pillar_size / 2.0 - wall_thickness,
            y: center_y - offset - pillar_size / 2.0,
            width: wall_thickness,
            height: pillar_size,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });

        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x - offset - pillar_size / 2.0,
            y: center_y + offset + pillar_size / 2.0 - wall_thickness,
            width: pillar_size,
            height: wall_thickness,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x - offset - pillar_size / 2.0,
            y: center_y + offset - pillar_size / 2.0,
            width: wall_thickness,
            height: pillar_size,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });

        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x + offset - pillar_size / 2.0,
            y: center_y + offset + pillar_size / 2.0 - wall_thickness,
            width: pillar_size,
            height: wall_thickness,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: center_x + offset + pillar_size / 2.0 - wall_thickness,
            y: center_y + offset - pillar_size / 2.0,
            width: wall_thickness,
            height: pillar_size,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });

        walls
    }

    fn create_team_bases_open() -> Vec<Wall> {
        let mut walls = Vec::new();
        let base_depth = 250.0;
        let base_width = 400.0;
        let wall_thickness = 20.0;

        let t1_base_x = WORLD_MIN_X + wall_thickness;
        let t1_base_y_center = 0.0;
        walls.push(Wall {
            id: generate_entity_id(),
            x: t1_base_x,
            y: t1_base_y_center - base_width / 2.0,
            width: wall_thickness,
            height: base_width,
            is_destructible: false,
            current_health: 1000,
            max_health: 1000,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: t1_base_x,
            y: t1_base_y_center - base_width / 2.0,
            width: base_depth * 0.6,
            height: wall_thickness,
            is_destructible: false,
            current_health: 700,
            max_health: 700,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: t1_base_x,
            y: t1_base_y_center + base_width / 2.0 - wall_thickness,
            width: base_depth * 0.6,
            height: wall_thickness,
            is_destructible: false,
            current_health: 700,
            max_health: 700,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: t1_base_x + base_depth * 0.3,
            y: t1_base_y_center - 50.0,
            width: 60.0,
            height: 25.0,
            is_destructible: true,
            current_health: 150,
            max_health: 150,
        });

        let t2_base_x = WORLD_MAX_X - base_depth - wall_thickness;
        let t2_base_y_center = 0.0;
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MAX_X - wall_thickness * 2.0,
            y: t2_base_y_center - base_width / 2.0,
            width: wall_thickness,
            height: base_width,
            is_destructible: false,
            current_health: 1000,
            max_health: 1000,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: t2_base_x + base_depth * 0.4 - wall_thickness,
            y: t2_base_y_center - base_width / 2.0,
            width: base_depth * 0.6,
            height: wall_thickness,
            is_destructible: false,
            current_health: 700,
            max_health: 700,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: t2_base_x + base_depth * 0.4 - wall_thickness,
            y: t2_base_y_center + base_width / 2.0 - wall_thickness,
            width: base_depth * 0.6,
            height: wall_thickness,
            is_destructible: false,
            current_health: 700,
            max_health: 700,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: t2_base_x + base_depth * 0.7 - 60.0,
            y: t2_base_y_center + 50.0,
            width: 60.0,
            height: 25.0,
            is_destructible: true,
            current_health: 150,
            max_health: 150,
        });

        walls
    }

    fn create_strategic_cover_sparse(
        rng: &mut impl Rng,
        number_of_cover_points: usize,
    ) -> Vec<Wall> {
        let mut walls = Vec::new();
        let cover_health = 120;

        for _ in 0..number_of_cover_points {
            let x = rng.gen_range(WORLD_MIN_X + 200.0..WORLD_MAX_X - 200.0);
            let y = rng.gen_range(WORLD_MIN_Y + 200.0..WORLD_MAX_Y - 200.0);

            if x.abs() < 250.0 && y.abs() < 250.0 {
                continue;
            }
            if !(WORLD_MIN_X + 400.0..=WORLD_MAX_X - 400.0).contains(&x) {
                continue;
            }

            let width = rng.gen_range(40.0..80.0);
            let height = rng.gen_range(15.0..30.0);
            walls.push(Wall {
                id: generate_entity_id(),
                x,
                y,
                width,
                height,
                is_destructible: true,
                current_health: cover_health,
                max_health: cover_health,
            });
        }
        walls
    }

    fn create_destructible_nodes_sparse(rng: &mut impl Rng, number_of_nodes: usize) -> Vec<Wall> {
        let mut walls = Vec::new();
        let node_health = 200;

        for _ in 0..number_of_nodes {
            let x = rng.gen_range(WORLD_MIN_X + 300.0..WORLD_MAX_X - 300.0);
            let y = rng.gen_range(WORLD_MIN_Y + 300.0..WORLD_MAX_Y - 300.0);

            if x.abs() < 150.0 && y.abs() < 150.0 {
                continue;
            }

            let size = rng.gen_range(50.0..70.0);
            walls.push(Wall {
                id: generate_entity_id(),
                x,
                y,
                width: size,
                height: size,
                is_destructible: true,
                current_health: node_health,
                max_health: node_health,
            });
        }
        walls
    }

    fn create_lanes_and_pathways(_rng: &mut impl Rng) -> Vec<Wall> {
        // Prefixed rng as it's not used in this version
        let mut walls = Vec::new();
        let wall_thickness = 15.0;
        let lane_wall_health = 300;

        let top_y_divider = WORLD_MIN_Y / 3.0;
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MIN_X + 300.0,
            y: top_y_divider,
            width: 400.0,
            height: wall_thickness,
            is_destructible: true,
            current_health: lane_wall_health,
            max_health: lane_wall_health,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MAX_X - 700.0,
            y: top_y_divider,
            width: 400.0,
            height: wall_thickness,
            is_destructible: true,
            current_health: lane_wall_health,
            max_health: lane_wall_health,
        });

        let bottom_y_divider = WORLD_MAX_Y / 3.0;
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MIN_X + 300.0,
            y: bottom_y_divider,
            width: 400.0,
            height: wall_thickness,
            is_destructible: true,
            current_health: lane_wall_health,
            max_health: lane_wall_health,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: WORLD_MAX_X - 700.0,
            y: bottom_y_divider,
            width: 400.0,
            height: wall_thickness,
            is_destructible: true,
            current_health: lane_wall_health,
            max_health: lane_wall_health,
        });

        let mid_x1 = -200.0;
        let mid_x2 = 200.0;
        walls.push(Wall {
            id: generate_entity_id(),
            x: mid_x1,
            y: WORLD_MIN_Y + 100.0,
            width: wall_thickness,
            height: 150.0,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: mid_x2,
            y: WORLD_MAX_Y - 250.0,
            width: wall_thickness,
            height: 150.0,
            is_destructible: false,
            current_health: 500,
            max_health: 500,
        });

        walls
    }

    /// "Corridors" - tight lanes, favors Shotgun/Melee
    pub fn generate_corridors_map(seed: u64) -> (Vec<Wall>, String) {
        let mut walls = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed ^ 0x000C_0771_D025);

        walls.extend(Self::create_border_walls());

        let wall_hp = 400;
        let corridor_width = 120.0;
        let wall_thickness = 20.0;

        // Create 3 horizontal corridors spanning the map
        for i in 0..3 {
            let y = WORLD_MIN_Y + (WORLD_MAX_Y - WORLD_MIN_Y) * (i as f32 + 1.0) / 4.0;
            // Long horizontal walls with periodic gaps
            let num_segments = 5;
            let segment_len = (WORLD_MAX_X - WORLD_MIN_X - 200.0) / (num_segments as f32 + 1.0);
            for s in 0..num_segments {
                let x = WORLD_MIN_X + 100.0 + (s as f32 + 0.5) * segment_len;
                walls.push(Wall {
                    id: generate_entity_id(),
                    x,
                    y: y - corridor_width / 2.0,
                    width: segment_len * 0.7,
                    height: wall_thickness,
                    is_destructible: false,
                    current_health: wall_hp,
                    max_health: wall_hp,
                });
                walls.push(Wall {
                    id: generate_entity_id(),
                    x,
                    y: y + corridor_width / 2.0,
                    width: segment_len * 0.7,
                    height: wall_thickness,
                    is_destructible: false,
                    current_health: wall_hp,
                    max_health: wall_hp,
                });
            }
        }

        // Add destructible cover blocks inside corridors
        for _ in 0..12 {
            let x = rng.gen_range(WORLD_MIN_X + 200.0..WORLD_MAX_X - 200.0);
            let y = rng.gen_range(WORLD_MIN_Y + 150.0..WORLD_MAX_Y - 150.0);
            walls.push(Wall {
                id: generate_entity_id(),
                x,
                y,
                width: rng.gen_range(30.0..50.0),
                height: rng.gen_range(30.0..50.0),
                is_destructible: true,
                current_health: 120,
                max_health: 120,
            });
        }

        (walls, "Corridors".to_string())
    }

    /// "Arena" - open center, favors Rifle/Sniper
    pub fn generate_arena_map(seed: u64) -> (Vec<Wall>, String) {
        let mut walls = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed ^ 0x000A_7E7A);

        walls.extend(Self::create_border_walls());

        // Large open center with pillars for cover
        let pillar_hp = 300;
        let pillar_size = 40.0;
        let ring_radius = 250.0;
        let num_pillars = 8;
        for i in 0..num_pillars {
            let angle = (i as f32 / num_pillars as f32) * std::f32::consts::TAU;
            let px = ring_radius * angle.cos() - pillar_size / 2.0;
            let py = ring_radius * angle.sin() - pillar_size / 2.0;
            walls.push(Wall {
                id: generate_entity_id(),
                x: px,
                y: py,
                width: pillar_size,
                height: pillar_size,
                is_destructible: false,
                current_health: pillar_hp,
                max_health: pillar_hp,
            });
        }

        // Outer ring of destructible barriers
        let outer_radius = 450.0;
        for i in 0..12 {
            let angle = (i as f32 / 12.0) * std::f32::consts::TAU + 0.1;
            let px = outer_radius * angle.cos();
            let py = outer_radius * angle.sin();
            walls.push(Wall {
                id: generate_entity_id(),
                x: px,
                y: py,
                width: rng.gen_range(50.0..90.0),
                height: rng.gen_range(15.0..25.0),
                is_destructible: true,
                current_health: 150,
                max_health: 150,
            });
        }

        // Small center obstacle
        walls.push(Wall {
            id: generate_entity_id(),
            x: -25.0,
            y: -25.0,
            width: 50.0,
            height: 50.0,
            is_destructible: true,
            current_health: 200,
            max_health: 200,
        });

        (walls, "Arena".to_string())
    }

    /// "Fortress" - asymmetric CTF, one team defends a fortified position
    pub fn generate_fortress_map(seed: u64) -> (Vec<Wall>, String) {
        let mut walls = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed ^ 0xF07_7E55);

        walls.extend(Self::create_border_walls());

        let fort_hp = 500;
        let thick = 20.0;

        // Fortress on the right side - walled compound
        let fx = 200.0;
        let fy = -200.0;
        let fw = 400.0;
        let fh = 400.0;

        // Fort walls with entrance gaps
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx,
            y: fy,
            width: fw,
            height: thick,
            is_destructible: false,
            current_health: fort_hp,
            max_health: fort_hp,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx,
            y: fy + fh - thick,
            width: fw,
            height: thick,
            is_destructible: false,
            current_health: fort_hp,
            max_health: fort_hp,
        });
        // Left wall with gap in middle
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx,
            y: fy,
            width: thick,
            height: fh * 0.35,
            is_destructible: false,
            current_health: fort_hp,
            max_health: fort_hp,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx,
            y: fy + fh * 0.65,
            width: thick,
            height: fh * 0.35,
            is_destructible: false,
            current_health: fort_hp,
            max_health: fort_hp,
        });
        // Right wall (solid)
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx + fw - thick,
            y: fy,
            width: thick,
            height: fh,
            is_destructible: false,
            current_health: fort_hp,
            max_health: fort_hp,
        });

        // Internal fort structures
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx + fw * 0.4,
            y: fy + fh * 0.3,
            width: 60.0,
            height: thick,
            is_destructible: true,
            current_health: 200,
            max_health: 200,
        });
        walls.push(Wall {
            id: generate_entity_id(),
            x: fx + fw * 0.4,
            y: fy + fh * 0.6,
            width: 60.0,
            height: thick,
            is_destructible: true,
            current_health: 200,
            max_health: 200,
        });

        // Approach cover (left side, for attackers)
        for _ in 0..8 {
            let x = rng.gen_range(WORLD_MIN_X + 100.0..fx - 50.0);
            let y = rng.gen_range(WORLD_MIN_Y + 150.0..WORLD_MAX_Y - 150.0);
            walls.push(Wall {
                id: generate_entity_id(),
                x,
                y,
                width: rng.gen_range(40.0..70.0),
                height: rng.gen_range(20.0..40.0),
                is_destructible: true,
                current_health: 100,
                max_health: 100,
            });
        }

        (walls, "Fortress".to_string())
    }

    /// Select a random map template based on a seed
    pub fn select_random_map(seed: u64, target_players: usize) -> (Vec<Wall>, String) {
        let template_index = (seed % 4) as usize;
        match template_index {
            0 => Self::generate_corridors_map(seed),
            1 => Self::generate_arena_map(seed),
            2 => Self::generate_fortress_map(seed),
            _ => Self::generate_dynamic_map_with_seed(target_players, seed),
        }
    }

    pub fn get_team_spawn_areas() -> Vec<(Vec2, u8)> {
        let mut spawns = Vec::new();
        let base_depth = 250.0;
        let base_width_half = 200.0;

        let t1_center_x = WORLD_MIN_X + base_depth * 0.5;
        let t1_center_y = 0.0;
        spawns.push((
            Vec2::new(t1_center_x, t1_center_y - base_width_half * 0.5),
            1,
        ));
        spawns.push((
            Vec2::new(t1_center_x, t1_center_y + base_width_half * 0.5),
            1,
        ));
        spawns.push((Vec2::new(t1_center_x + 50.0, t1_center_y), 1));
        spawns.push((
            Vec2::new(t1_center_x - 50.0, t1_center_y - base_width_half * 0.25),
            1,
        ));
        spawns.push((
            Vec2::new(t1_center_x - 50.0, t1_center_y + base_width_half * 0.25),
            1,
        ));

        let t2_center_x = WORLD_MAX_X - base_depth * 0.5;
        let t2_center_y = 0.0;
        spawns.push((
            Vec2::new(t2_center_x, t2_center_y - base_width_half * 0.5),
            2,
        ));
        spawns.push((
            Vec2::new(t2_center_x, t2_center_y + base_width_half * 0.5),
            2,
        ));
        spawns.push((Vec2::new(t2_center_x - 50.0, t2_center_y), 2));
        spawns.push((
            Vec2::new(t2_center_x + 50.0, t2_center_y - base_width_half * 0.25),
            2,
        ));
        spawns.push((
            Vec2::new(t2_center_x + 50.0, t2_center_y + base_width_half * 0.25),
            2,
        ));

        spawns.push((Vec2::new(WORLD_MIN_X + 100.0, WORLD_MAX_Y - 100.0), 0));
        spawns.push((Vec2::new(WORLD_MAX_X - 100.0, WORLD_MIN_Y + 100.0), 0));

        spawns
    }
}
