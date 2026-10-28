//! Seeded ceiling fixtures shared by the client and organizer replay model.
#[allow(unused_imports)]
use crate::prelude::*;
use crate::world::{Hall, Map};

fn hash(x: usize, z: usize, seed: u64) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x1f123bb5)
        ^ (z as u32).wrapping_mul(0x5f356495)
        ^ seed as u32
        ^ (seed >> 32) as u32;
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846ca68b);
    h ^ (h >> 16)
}

pub(crate) fn grid_active(x: usize, z: usize, seed: u64, spawn: [usize; 2]) -> bool {
    [x, z] == spawn
        || (!((x * 37 + z * 19) as u64 ^ seed).is_multiple_of(7) && hash(x, z, seed) % 10 >= 3)
}

fn corner(map: &Map, x: usize, z: usize) -> bool {
    let (x, z) = (x as i32, z as i32);
    let n = map.wall(x, z - 1);
    let e = map.wall(x + 1, z);
    let s = map.wall(x, z + 1);
    let w = map.wall(x - 1, z);
    // Exactly two orthogonal walls: a genuine bend, never a straight corridor.
    (n != s) && (e != w)
}

fn room(map: &Map, hall: &Hall, out: &mut [u8]) {
    let [x0, z0] = hall.min;
    let [w, h] = hall.size;
    let seed = hall.light_seed;
    let phase = hash(x0, z0, seed) as usize;
    for z in z0..(z0 + h).min(map.height()) {
        for x in x0..(x0 + w).min(map.width()) {
            if map.wall(x as i32, z as i32) {
                continue;
            }
            let (mut u, mut v) = (x - x0, z - z0);
            if phase & 1 != 0 {
                core::mem::swap(&mut u, &mut v);
            }
            u += phase % 7;
            v += (phase >> 4) % 7;
            let on = match hall.light_profile {
                0 => false,
                // Broken rows, staggered panels, short clusters, paired strips,
                // and offset columns use room-local phases, never global rows.
                1 => u % 5 == 0 && v % 3 != 2,
                2 => (u + 2 * v) % 4 == 0 && (u / 4 + v / 2) % 2 == 0,
                3 => u % 5 < 2 && v % 5 < 2,
                4 => v % 6 < 2 && u % 3 == 0,
                5 => (u + v / 3 * 2) % 5 == 0 && v % 3 != 2,
                6 => (u + v / 4 * 3) % 6 < 2 && v % 4 == 0,
                _ => (x + 2 * z) % 4 == 0 && grid_active(x, z, map.seed, [usize::MAX; 2]),
            };
            let active = on && (hall.light_profile == 7 || !hash(x, z, seed).is_multiple_of(10));
            out[z * map.width() + x] = if active {
                2
            } else {
                u8::from((x + 2 * z) % 4 == 0)
            };
        }
    }
    if hall.light_profile != 0 {
        return;
    }
    // Maze rooms light one or two well-separated bends in the generated walls.
    let mut first = None;
    for _ in 0..1 + (phase >> 8) % 2 {
        let mut best = None;
        for z in z0..(z0 + h).min(map.height()) {
            for x in x0..(x0 + w).min(map.width()) {
                if !map.wall(x as i32, z as i32)
                    && corner(map, x, z)
                    && first.is_none_or(|[a, b]: [usize; 2]| x.abs_diff(a) + z.abs_diff(b) >= 5)
                {
                    let key = hash(x, z, seed);
                    if best.is_none_or(|(score, _, _)| key < score) {
                        best = Some((key, x, z));
                    }
                }
            }
        }
        if let Some((_, x, z)) = best {
            out[z * map.width() + x] = 2;
            first = Some([x, z]);
        }
    }
}

/// Per cell: 0 = no fixture, 1 = unlit fixture, 2 = emitting fixture.
/// Extra fixtures outside the original grid are always emitting, allowing a
/// replay to reconstruct both fixture kinds from the compact active bitmap.
pub fn layout(map: &Map) -> Vec<u8> {
    let spawn = [
        (map.spawn.x / map.tile_size) as usize,
        (map.spawn.z / map.tile_size) as usize,
    ];
    let mut out = vec![0; map.width() * map.height()];
    for z in 0..map.height() {
        for x in 0..map.width() {
            if !map.wall(x as i32, z as i32) && (x + 2 * z) % 4 == 0 {
                out[z * map.width() + x] = 1 + u8::from(grid_active(x, z, map.seed, spawn));
            }
        }
    }
    for hall in &map.halls {
        room(map, hall, &mut out);
    }
    if spawn[0] < map.width() && spawn[1] < map.height() {
        out[spawn[1] * map.width() + spawn[0]] = 2;
    }
    out
}

/// Active lights, LSB-first, with the same cell ordering as the map bitmap.
pub fn bits(map: &Map) -> Vec<u8> {
    let mut bits = vec![0; (map.width() * map.height()).div_ceil(8)];
    for (i, cell) in layout(map).into_iter().enumerate() {
        if cell == 2 {
            bits[i / 8] |= 1 << (i % 8);
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maze_rooms_only_light_one_or_two_real_bends() {
        let (mut rooms, mut lit, mut floor) = (0, 0, 0);
        let mut profiles = 0u8;
        for seed in (0..16).chain([42, 2026, 1 << 40, u64::MAX]) {
            let map = Map::procedural(seed).unwrap();
            let lights = layout(&map);
            let spawn = [
                (map.spawn.x / map.tile_size) as usize,
                (map.spawn.z / map.tile_size) as usize,
            ];
            for hall in &map.halls {
                profiles |= 1 << hall.light_profile;
                if hall.light_profile != 0 {
                    continue;
                }
                rooms += 1;
                let mut count = 0;
                let [x0, z0] = hall.min;
                for z in z0..z0 + hall.size[1] {
                    for x in x0..x0 + hall.size[0] {
                        floor += usize::from(!map.wall(x as i32, z as i32));
                        if lights[z * map.width() + x] == 2 && [x, z] != spawn {
                            assert!(corner(&map, x, z), "seed {seed}, ({x}, {z})");
                            count += 1;
                        }
                    }
                }
                assert!(count <= 2, "seed {seed}, {count} active maze lights");
                lit += count;
            }
        }
        assert_eq!(profiles, 127, "all seven terrain styles must occur");
        assert!(rooms > 100 && lit >= rooms);
        assert!(lit * 30 < floor, "maze areas must remain mostly unlit");
    }

    #[test]
    fn layout_and_bitmap_are_repeatable_without_mutating_gameplay() {
        for seed in [0, 42, 2026, 987654321, 1 << 40, u64::MAX] {
            let map = Map::procedural(seed).unwrap();
            let before = map.clone();
            let lights = layout(&map);
            let encoded = bits(&map);
            assert_eq!(encoded.len(), 8192);
            assert_eq!(lights, layout(&Map::procedural(seed).unwrap()));
            assert_eq!(map.walls, before.walls);
            assert_eq!(map.spawn.x, before.spawn.x);
            assert_eq!(map.spawn.z, before.spawn.z);
            assert_eq!(map.exit, before.exit);
            assert_eq!(map.entities, before.entities);
            for (i, &cell) in lights.iter().enumerate() {
                let (x, z) = (i % map.width(), i / map.width());
                assert_eq!(encoded[i / 8] >> (i % 8) & 1 != 0, cell == 2);
                assert!(cell == 0 || !map.wall(x as i32, z as i32));
                // Replay reconstructs unlit fixtures from the original grid.
                assert!(cell != 1 || (x + 2 * z) % 4 == 0);
                if !map.wall(x as i32, z as i32) && (x + 2 * z) % 4 == 0 {
                    assert_ne!(cell, 0);
                }
            }
            let spawn = (map.spawn.z / map.tile_size) as usize * map.width()
                + (map.spawn.x / map.tile_size) as usize;
            assert_eq!(lights[spawn], 2);
        }
    }

    #[test]
    fn room_formulas_have_distinct_and_seeded_arrangements() {
        let mut map = Map::generated(33, 1).unwrap();
        for z in 0..33 {
            for x in 0..33 {
                map.set_wall(x, z, x == 0 || z == 0 || x == 32 || z == 32);
            }
        }
        map.halls = vec![Hall {
            min: [1, 1],
            size: [31, 31],
            doors: vec![],
            light_profile: 1,
            light_seed: 42,
        }];
        let mut layouts = vec![];
        for profile in 1..7 {
            map.halls[0].light_profile = profile;
            let a = bits(&map);
            assert!(layouts.iter().all(|b| *b != a));
            layouts.push(a.clone());
            map.halls[0].light_seed = 2026;
            assert_ne!(a, bits(&map));
            map.halls[0].light_seed = 42;
        }
    }
}
