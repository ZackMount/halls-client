use halls::world::Map;
use halls::{entity::spawn_with_seed, player::Player};

#[test]
fn seeds_change_layout_and_player_spawn_but_reproduce_exactly() {
    let maps: Vec<_> = [104729, 220033, 730201]
        .into_iter()
        .map(|seed| Map::procedural(seed).unwrap())
        .collect();
    for pair in maps.windows(2) {
        assert_ne!(pair[0].walls, pair[1].walls);
        assert!((pair[0].spawn.x - pair[1].spawn.x).hypot(pair[0].spawn.z - pair[1].spawn.z) > 1.0);
    }
    let again = Map::procedural(104729).unwrap();
    assert_eq!(maps[0].walls, again.walls);
    assert_eq!(maps[0].spawn.x, again.spawn.x);
    assert_eq!(maps[0].spawn.z, again.spawn.z);
}

#[test]
fn fixed_size_keeps_borders_and_full_width_routes_valid() {
    for seed in 0..32 {
        let map = Map::procedural(seed).unwrap();
        assert_eq!((map.width(), map.height()), (256, 256));
        for edge in 0..256 {
            assert!(map.wall(edge, 0) && map.wall(edge, 255));
            assert!(map.wall(0, edge) && map.wall(255, edge));
        }
        let distances = map.distances();
        let start = [
            (map.spawn.x / map.tile_size) as usize,
            (map.spawn.z / map.tile_size) as usize,
        ];
        let route = halls::entity::path(&map, start, map.exit);
        assert_eq!(
            route.len(),
            distances[map.exit[1] * 256 + map.exit[0]] as usize
        );
        #[cfg(feature = "tools")]
        assert_eq!(route.len(), map.shortest_route().unwrap().len());
        let last = (0..distances.len())
            .rev()
            .find(|&i| distances[i] != u16::MAX)
            .unwrap();
        assert!(last > 32767);
        let route = halls::entity::path(&map, start, [last % 256, last / 256]);
        assert_eq!(route.len(), distances[last] as usize);
        assert_eq!(route.back(), Some(&(last as u16)));
        assert_eq!(spawn_with_seed(&map, seed).len(), 5);
    }
}

#[test]
fn actual_entities_have_safe_distributed_spawns_and_keep_patrolling() {
    for seed in [1, 42, 104729, 220033, 730201] {
        let map = Map::procedural(seed).unwrap();
        let distances = map.distances();
        let mut starts = std::collections::HashSet::new();
        for run in 0..8 {
            let enemies = spawn_with_seed(&map, run);
            assert_eq!(enemies.len(), 5);
            let cells: Vec<_> = enemies
                .iter()
                .map(|enemy| {
                    assert!(map.walkable(enemy.position[0], enemy.position[1], 0.0));
                    assert!(
                        (enemy.position[0] - map.spawn.x).hypot(enemy.position[1] - map.spawn.z)
                            >= 64.0
                    );
                    let cell = [
                        (enemy.position[0] / map.tile_size) as usize,
                        (enemy.position[1] / map.tile_size) as usize,
                    ];
                    assert!(
                        distances[cell[1] * map.width() + cell[0]] as f32 * map.tile_size >= 96.0
                    );
                    cell
                })
                .collect();
            for i in 0..cells.len() {
                for j in 0..i {
                    assert!(
                        cells[i][0].abs_diff(cells[j][0]) + cells[i][1].abs_diff(cells[j][1]) >= 32
                    );
                }
            }
            starts.insert(cells);
        }
        assert!(starts.len() > 1);
        let mut enemies = spawn_with_seed(&map, 42);
        let local_patrols: Vec<_> = enemies
            .iter()
            .map(|enemy| enemy.patrol_anchor.is_some())
            .collect();
        let mut player = Player::new(&map);
        // Isolate autonomous patrol from legitimate visual detection of a player.
        player.camera.position.x = -500.0;
        player.camera.position.z = -500.0;
        let mut pauses = [0.0f32; 5];
        let mut longest = [0.0f32; 5];
        for frame in 0..2400 {
            for (i, enemy) in enemies.iter_mut().enumerate() {
                let before = enemy.position;
                enemy.tick(&map, &player, frame as f32 * 0.05, 0.05, 0.0, true);
                assert!(map.walkable(enemy.position[0], enemy.position[1], 0.0));
                if local_patrols[i]
                    && let Some(anchor) = enemy.patrol_anchor
                {
                    let x = (anchor[0] as f32 + 0.5) * map.tile_size;
                    let z = (anchor[1] as f32 + 0.5) * map.tile_size;
                    assert!((enemy.position[0] - x).abs() + (enemy.position[1] - z).abs() <= 16.1);
                }
                if enemy.position == before {
                    pauses[i] += 0.05;
                    longest[i] = longest[i].max(pauses[i]);
                } else {
                    pauses[i] = 0.0;
                }
            }
        }
        for (i, enemy) in enemies.iter().enumerate() {
            assert!(
                enemy.travel > 70.0,
                "seed {seed}: patrol covered only {}m",
                enemy.travel
            );
            assert!(
                longest[i] < 10.0,
                "seed {seed}: patrol stood for {}s",
                longest[i]
            );
        }
    }
}

#[test]
fn five_patrols_cover_quadrants_and_a_real_route_middle_without_changing_geometry() {
    for seed in (0..64).chain([104729, 220033, 730201, u64::MAX]) {
        let map = Map::procedural(seed).unwrap();
        assert_eq!(map.entities.len(), 5, "seed {seed}");
        let quadrants: std::collections::HashSet<_> = map.entities[..4]
            .iter()
            .map(|c| (c[0] / 128, c[1] / 128))
            .collect();
        assert_eq!(quadrants.len(), 4, "seed {seed}");
        let center = map.entities[4];
        let from_spawn = map.distances();
        let mut reverse = map.clone();
        reverse.spawn.x = (map.exit[0] as f32 + 0.5) * map.tile_size;
        reverse.spawn.z = (map.exit[1] as f32 + 0.5) * map.tile_size;
        let from_exit = reverse.distances();
        let total = from_spawn[map.exit[1] * 256 + map.exit[0]];
        let index = center[1] * 256 + center[0];
        assert_eq!(
            from_spawn[index] as u32 + from_exit[index] as u32,
            total as u32,
            "seed {seed}"
        );
        assert!(from_spawn[index] >= total / 3 && from_spawn[index] <= total - total / 3);
        for run in [0, 17] {
            let patrols = spawn_with_seed(&map, run);
            assert_eq!(
                patrols.iter().filter(|e| e.patrol_anchor.is_some()).count(),
                1
            );
            let anchored = patrols.iter().find(|e| e.patrol_anchor.is_some()).unwrap();
            assert_eq!(anchored.patrol_anchor, Some(center));
        }
        assert_eq!(map.entities, Map::procedural(seed).unwrap().entities);
    }
}

#[test]
fn room_grammar_has_stable_maps_and_no_isolated_floor() {
    for (seed, expected, x, z, exit) in [
        (42u64, 10882297506545850457u64, 377.0, 121.0, [54, 226]),
        (2026u64, 10785059042717215436u64, 243.0, 379.0, [1, 1]),
        (
            987654321u64,
            17238968053346401619u64,
            353.0,
            61.0,
            [173, 222],
        ),
        (
            18446744073709551615u64,
            4387449179045488007u64,
            259.0,
            93.0,
            [197, 248],
        ),
    ] {
        let map = Map::procedural(seed).unwrap();
        let distances = map.distances();
        let mut hash = 14695981039346656037u64;
        for z in 0..256 {
            for x in 0..256 {
                let wall = map.wall(x, z);
                hash = (hash ^ u64::from(wall)).wrapping_mul(1099511628211);
                assert!(wall || distances[z as usize * 256 + x as usize] != u16::MAX);
            }
        }
        assert_eq!(hash, expected, "seed {seed}");
        assert_eq!((map.spawn.x, map.spawn.z), (x, z));
        assert_eq!(map.exit, exit);
        assert!(map.halls.len() > 100);
    }
}

#[cfg(feature = "tools")]
#[test]
fn every_generated_floor_cell_is_reachable_with_body_clearance() {
    use std::collections::VecDeque;

    for seed in [0, 42, 2026, 104729, 987654321, u64::MAX] {
        let map = Map::procedural(seed).unwrap();
        assert!(map.blocks.is_empty() && map.elevations.is_empty());
        let width = map.width();
        let tile = map.tile_size;
        let start = (map.spawn.z / tile) as usize * width + (map.spawn.x / tile) as usize;
        let mut visited = vec![false; width * map.height()];
        let mut queue = VecDeque::from([start]);
        visited[start] = true;
        while let Some(at) = queue.pop_front() {
            let (x, z) = ((at % width) as i32, (at / width) as i32);
            for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let (nx, nz) = (x + dx, z + dz);
                if map.wall(nx, nz) {
                    continue;
                }
                let next = nz as usize * width + nx as usize;
                if visited[next] {
                    continue;
                }
                // Test the physical crossing, not merely the floor bitmap.
                // Current player radius is 0.22 m; the entity body is 0.26 m.
                for sample in 0..=16 {
                    let phase = sample as f32 / 16.0;
                    let px = (x as f32 + 0.5 + dx as f32 * phase) * tile;
                    let pz = (z as f32 + 0.5 + dz as f32 * phase) * tile;
                    assert!(map.walkable(px, pz, 0.0), "player crossing, seed={seed}");
                    for (ox, oz) in [(-0.26, -0.26), (-0.26, 0.26), (0.26, -0.26), (0.26, 0.26)] {
                        assert!(
                            !map.wall(
                                ((px + ox) / tile).floor() as i32,
                                ((pz + oz) / tile).floor() as i32
                            ),
                            "entity crossing, seed={seed}"
                        );
                    }
                }
                visited[next] = true;
                queue.push_back(next);
            }
        }
        for (index, &reachable) in visited.iter().enumerate() {
            if !map.wall((index % width) as i32, (index / width) as i32) {
                assert!(reachable, "sealed floor pocket, seed={seed}, cell={index}");
            }
        }
    }
}
