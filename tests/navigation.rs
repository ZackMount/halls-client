use halls::{player::Player, world::Map};

#[test]
fn canonical_routes_are_playable_through_the_same_collision_system() {
    let maps = vec![Map::generated(17, 42).unwrap()];
    for map in maps {
        let route = map.shortest_route().unwrap();
        let mut player = Player::new(&map);
        for direction in route {
            player.camera.yaw = match direction {
                b'N' => std::f32::consts::PI,
                b'E' => std::f32::consts::FRAC_PI_2,
                b'S' => 0.0,
                b'W' => std::f32::consts::PI * 1.5,
                _ => unreachable!(),
            };
            player.move_by(&map, map.tile_size, 0.0);
        }
        assert!(player.won);
    }
}
