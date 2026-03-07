use massive_game_server_core::world::map_generator::MapGenerator;

#[test]
fn named_templates_generate_non_empty_wall_sets() {
    let (corridors_walls, corridors_name) = MapGenerator::generate_corridors_map(11);
    let (arena_walls, arena_name) = MapGenerator::generate_arena_map(22);
    let (fortress_walls, fortress_name) = MapGenerator::generate_fortress_map(33);
    let (random_walls, random_name) = MapGenerator::select_random_map(44, 32);

    assert!(!corridors_walls.is_empty());
    assert!(!arena_walls.is_empty());
    assert!(!fortress_walls.is_empty());
    assert!(!random_walls.is_empty());

    assert!(corridors_name.to_ascii_lowercase().contains("corridor"));
    assert!(arena_name.to_ascii_lowercase().contains("arena"));
    assert!(fortress_name.to_ascii_lowercase().contains("fortress"));
    assert!(!random_name.trim().is_empty());
}
