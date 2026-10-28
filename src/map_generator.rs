#[allow(unused_imports)]
use crate::prelude::*;
// Deterministic procedural rooms for the shipping map.
use crate::{
    bitmap::Walls,
    error::Result,
    world::{Hall, Map, Spawn},
};
pub fn generate(seed: u64) -> Result<Map> {
    generate_observed(seed, |_, _, _, _| {})
}
pub fn generate_observed(
    seed: u64,
    mut observe: impl FnMut([usize; 4], usize, Option<usize>, u64),
) -> Result<Map> {
    use crate::map_grammar::random;
    let mut map = Map {
        #[cfg(any(feature = "tools", test))]
        version: 2,
        walls: Walls::new(256, 256),
        spawn: Spawn {
            x: 3.,
            z: 3.,
            yaw: 0.,
        },
        exit: [1, 1],
        tile_size: 2.,
        ceiling: 2.65,
        eye_height: 1.55,
        #[cfg(any(feature = "tools", test))]
        elevations: vec![],
        #[cfg(any(feature = "tools", test))]
        blocks: vec![],
        seed,
        #[cfg(any(feature = "tools", test))]
        breakers: vec![],
        entities: vec![],
        halls: vec![],
    };
    let mut state = seed ^ 0xa0761d6478bd642f;
    let mut regions = vec![[1usize, 1, 254, 254]];
    let mut leaves = vec![];
    let mut anchors = vec![];
    let mut families: Vec<u16> = vec![];
    let mut counts = [0usize; 32];
    while let Some([x, z, w, h]) = regions.pop() {
        let along_x = if w == h {
            random(&mut state) & 1 == 0
        } else {
            w > h
        };
        let span = if along_x { w } else { h };
        if span > 25 + random(&mut state) as usize % 10 {
            let cut = span * (40 + random(&mut state) as usize % 21) / 100;
            if along_x {
                regions.push([x + cut, z, w - cut, h]);
                regions.push([x, z, cut, h]);
            } else {
                regions.push([x, z + cut, w, h - cut]);
                regions.push([x, z, w, cut]);
            }
            continue;
        }
        leaves.push([x, z, w, h]);
    }
    let mut remaining = leaves.len();
    let mut mixed_left = remaining / 5;
    for [x, z, w, h] in leaves {
        let mixed = random(&mut state) as usize % remaining < mixed_left;
        remaining -= 1;
        mixed_left -= usize::from(mixed);
        // Shared thin boundaries use the whole leaf; no wide padding between rooms.
        let (rx, rz, rw, rh) = (x, z, w - 1, h - 1);
        // Score all formula choices against actual spatial neighbours, not
        // merely the previous item in traversal order. Randomize equal scores.
        let (mut exact, mut adjacent, mut nearby) = (0u32, 0u32, 0u32);
        for (j, hall) in map.halls.iter().enumerate() {
            let gx = rx
                .saturating_sub(hall.min[0] + hall.size[0])
                .max(hall.min[0].saturating_sub(rx + rw));
            let gz = rz
                .saturating_sub(hall.min[1] + hall.size[1])
                .max(hall.min[1].saturating_sub(rz + rh));
            let primary = (families[j] & 31) as usize;
            let mut group = 1 << crate::map_grammar::style(primary);
            let other = (families[j] >> 5) as usize;
            if other > 0 {
                let f = other - 1;
                group |= 1 << crate::map_grammar::style(f);
                if gx + gz <= 1 {
                    exact |= 1 << f;
                }
            }
            if gx + gz <= 1 {
                exact |= 1 << primary;
                adjacent |= group;
            }
            if gx + gz <= 12 {
                nearby |= group;
            }
        }
        let masks = [exact, adjacent, nearby];
        let family = crate::map_grammar::choose(&counts, masks, usize::MAX, &mut state);
        counts[family] += 1;
        let secondary = if mixed {
            let f = crate::map_grammar::choose(
                &counts,
                masks,
                crate::map_grammar::style(family),
                &mut state,
            );
            counts[f] += 1;
            Some(f)
        } else {
            None
        };
        families.push(family as u16 | ((secondary.map_or(0, |f| f + 1) as u16) << 5));
        let flip = random(&mut state);
        let room_seed = random(&mut state);
        observe([rx, rz, rw, rh], family, secondary, room_seed);
        let cells = if let Some(other) = secondary {
            crate::map_grammar::blend(rw, rh, family, other, room_seed)
        } else {
            crate::map_grammar::tile(rw, rh, family, room_seed)
        };
        for v in 0..rh {
            for u in 0..rw {
                map.set_wall(
                    rx + if flip & 1 == 0 { u } else { rw - 1 - u },
                    rz + if flip & 2 == 0 { v } else { rh - 1 - v },
                    cells[v * rw + u] != 0,
                );
            }
        }
        let anchor = [
            rx + if flip & 1 == 0 { 1 } else { rw - 2 },
            rz + if flip & 2 == 0 { 1 } else { rh - 2 },
        ];
        anchors.push(anchor);
        map.halls.push(Hall {
            min: [rx, rz],
            size: [rw, rh],
            doors: vec![anchor],
            // Lighting uses a separate seed so it cannot change floor geometry.
            light_profile: if secondary.is_some_and(|f| crate::map_grammar::style(f) == 0) {
                0
            } else {
                crate::map_grammar::style(family) as u8
            },
            light_seed: room_seed,
        });
    }
    // Connect each region to its nearest earlier neighbour. A second local link
    // occasionally adds a loop; all rooms retain a guaranteed connected anchor.
    for i in 1..anchors.len() {
        let mut nearest = [(usize::MAX, 0); 2];
        for j in 0..i {
            let d = anchors[i][0].abs_diff(anchors[j][0]) + anchors[i][1].abs_diff(anchors[j][1]);
            if d < nearest[0].0 {
                nearest[1] = nearest[0];
                nearest[0] = (d, j);
            } else if d < nearest[1].0 {
                nearest[1] = (d, j);
            }
        }
        let count = 1 + usize::from(i > 1 && random(&mut state).is_multiple_of(3));
        for &(_, j) in &nearest[..count] {
            let a = anchors[i];
            let b = anchors[j];
            let roll = random(&mut state) % 10;
            let width = 1 + usize::from(roll >= 3) + usize::from(roll >= 9);
            let elbow = if random(&mut state) & 1 == 0 {
                [a[0], b[1]]
            } else {
                [b[0], a[1]]
            };
            for (from, to) in [(a, elbow), (elbow, b)] {
                for z in from[1].min(to[1])..=from[1].max(to[1]) {
                    for x in from[0].min(to[0])..=from[0].max(to[0]) {
                        for dz in 0..width {
                            for dx in 0..width {
                                map.set_wall(x + dx - width / 2, z + dz - width / 2, false);
                            }
                        }
                    }
                }
            }
        }
    }
    map.spawn.x = (anchors[0][0] as f32 + 0.5) * 2.;
    map.spawn.z = (anchors[0][1] as f32 + 0.5) * 2.;
    map.randomize_player_spawn(seed)?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    #[test]
    fn exactly_one_fifth_of_rooms_blend_distinct_formula_styles() {
        for seed in 0..32 {
            let (mut rooms, mut mixed) = (0, 0);
            super::generate_observed(seed, |_, first, second, _| {
                rooms += 1;
                if let Some(other) = second {
                    mixed += 1;
                    assert_ne!(
                        crate::map_grammar::style(first),
                        crate::map_grammar::style(other)
                    );
                }
            })
            .unwrap();
            assert_eq!(mixed, rooms / 5);
        }
    }
}
