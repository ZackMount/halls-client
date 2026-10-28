//! Optional objectives. Placement and cell-entry collection are reproduced by the judge.
#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    math::{Vec2, Vec3},
    mesh::{Material, Mesh, Triangle, Vertex},
    world::Map,
};
use alloc::collections::VecDeque;

pub const COUNT: usize = 3;
const EFFECT_SECONDS: f32 = 0.75;

fn distances(map: &Map, start: usize) -> Vec<u32> {
    let width = map.width();
    let mut result = vec![u32::MAX; width * map.height()];
    result[start] = 0;
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        let (x, z) = ((at % width) as i32, (at / width) as i32);
        for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            if !map.wall(x + dx, z + dz) {
                let next = (z + dz) as usize * width + (x + dx) as usize;
                if result[next] == u32::MAX {
                    result[next] = result[at] + 1;
                    queue.push_back(next);
                }
            }
        }
    }
    result
}

fn tie(seed: u64, index: usize) -> u64 {
    let mut v = seed ^ (index as u64).wrapping_mul(0x9e3779b97f4a7c15);
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}

pub fn positions(map: &Map) -> Vec<[usize; 2]> {
    let width = map.width();
    let start =
        (map.spawn.z / map.tile_size) as usize * width + (map.spawn.x / map.tile_size) as usize;
    let end = map.exit[1] * width + map.exit[0];
    if start >= width * map.height() || end >= width * map.height() {
        return vec![];
    }
    let from_start = distances(map, start);
    let from_exit = distances(map, end);
    let direct = from_start[end];
    if direct == u32::MAX {
        return vec![];
    }
    let mut candidates = vec![];
    for margin in [8, 2, 1] {
        candidates = (0..from_start.len())
            .filter(|&i| {
                from_start[i] != u32::MAX
                    && from_exit[i] != u32::MAX
                    && from_start[i] >= margin
                    && from_exit[i] >= margin
            })
            .collect();
        if candidates.len() >= COUNT {
            break;
        }
    }
    let Some(first) = candidates.iter().copied().min_by_key(|&i| {
        let detour = from_start[i] + from_exit[i] - direct;
        (
            (3 * from_start[i]).abs_diff(direct) + 4 * detour,
            tie(map.seed, i),
            i,
        )
    }) else {
        return vec![];
    };
    let mut selected = vec![first];
    let mut separation: Vec<_> = from_start
        .iter()
        .zip(&from_exit)
        .map(|(&a, &b)| a.min(b))
        .collect();
    while selected.len() < COUNT && selected.len() < candidates.len() {
        let last = distances(map, *selected.last().unwrap());
        for (value, &distance) in separation.iter_mut().zip(&last) {
            *value = (*value).min(distance);
        }
        let next = candidates
            .iter()
            .copied()
            .filter(|i| !selected.contains(i))
            .min_by_key(|&i| {
                (
                    u32::MAX - separation[i],
                    u32::MAX - (from_start[i] + from_exit[i] - direct),
                    tie(map.seed, i),
                    i,
                )
            })
            .unwrap();
        selected.push(next);
    }
    selected
        .into_iter()
        .map(|i| [i % width, i / width])
        .collect()
}

pub struct Coins {
    pub cells: Vec<[usize; 2]>,
    pub collected_mask: u8,
    pickup_age: [f32; COUNT],
    clock: f32,
}
impl Coins {
    pub fn new(map: &Map) -> Self {
        Self {
            cells: positions(map),
            collected_mask: 0,
            pickup_age: [EFFECT_SECONDS; COUNT],
            clock: 0.0,
        }
    }
    pub fn collected(&self) -> usize {
        self.collected_mask.count_ones() as usize
    }
    /// Matches the half-open objective cells used by the server. No extra input is required.
    pub fn tick(&mut self, map: &Map, position: Vec3, dt: f32, alive: bool) -> bool {
        self.clock += dt;
        for age in &mut self.pickup_age {
            *age = (*age + dt).min(EFFECT_SECONDS);
        }
        if !alive || position.x < 0.0 || position.z < 0.0 {
            return false;
        }
        let cell = [
            (position.x / map.tile_size) as usize,
            (position.z / map.tile_size) as usize,
        ];
        let mut picked = false;
        for (i, &at) in self.cells.iter().enumerate() {
            if at == cell && self.collected_mask & (1 << i) == 0 {
                self.collected_mask |= 1 << i;
                self.pickup_age[i] = 0.0;
                picked = true;
            }
        }
        picked
    }
    pub fn draw(&self, map: &Map, mesh: &mut Mesh, camera: Vec3) {
        for (i, cell) in self.cells.iter().enumerate() {
            let x = (cell[0] as f32 + 0.5) * map.tile_size;
            let z = (cell[1] as f32 + 0.5) * map.tile_size;
            if (x - camera.x).powi(2) + (z - camera.z).powi(2) > 55.0 * 55.0 {
                continue;
            }
            let at = Vec3::new(x, map.floor_at(x, z) + 1.05, z);
            let phase = self.clock * 1.55 + i as f32 * 1.7;
            if self.collected_mask & (1 << i) == 0 {
                draw_coin(
                    mesh,
                    at + Vec3::new(0.0, (self.clock * 2.3 + i as f32).sin() * 0.045, 0.0),
                    phase,
                    1.0,
                );
            } else {
                let age = self.pickup_age[i];
                if age < EFFECT_SECONDS {
                    let t = age / EFFECT_SECONDS;
                    if t < 0.48 {
                        let s = (1.0 - t / 0.48).sqrt();
                        draw_coin(mesh, at + Vec3::new(0.0, t * 0.38, 0.0), phase + t * 7.0, s);
                    }
                    for j in 0..12 {
                        let angle = j as f32 * core::f32::consts::TAU / 12.0 + i as f32;
                        let speed = 0.60 + (j % 3) as f32 * 0.14;
                        let p = at
                            + Vec3::new(
                                angle.cos() * t * speed,
                                t * (0.45 + (j % 2) as f32 * 0.16) - t * t * 0.32,
                                angle.sin() * t * speed,
                            );
                        let size = 0.026 * (1.0 - t).sqrt();
                        sparkle(mesh, p, size, (1.0 - t) * 1.1);
                    }
                }
            }
        }
    }
}

fn triangle(mesh: &mut Mesh, points: [Vec3; 3], material: Material, light: f32) {
    let normal = (points[1] - points[0])
        .cross(points[2] - points[0])
        .normalized();
    let light = light * (0.60 + 0.40 * (normal.x * -0.3 + normal.y * 0.5 + normal.z * -0.8).abs());
    mesh.triangles.push(Triangle {
        vertices: points.map(|p| Vertex {
            p,
            uv: Vec2::default(),
            light,
        }),
        normal,
        material,
        lightmap: u32::MAX,
    });
}

fn draw_coin(mesh: &mut Mesh, at: Vec3, angle: f32, scale: f32) {
    let right = Vec3::new(angle.cos(), 0.0, -angle.sin());
    let normal = Vec3::new(angle.sin(), 0.0, angle.cos());
    let up = Vec3::new(0.0, 1.0, 0.0);
    let point = |r: f32, a: f32, depth: f32| {
        at + (right * (a.cos() * r) + up * (a.sin() * r) + normal * depth) * scale
    };
    const SEGMENTS: usize = 20;
    for i in 0..SEGMENTS {
        let a = i as f32 * core::f32::consts::TAU / SEGMENTS as f32;
        let b = (i + 1) as f32 * core::f32::consts::TAU / SEGMENTS as f32;
        let p = [
            point(0.23, a, -0.027),
            point(0.23, b, -0.027),
            point(0.23, b, 0.027),
            point(0.23, a, 0.027),
        ];
        triangle(mesh, [p[0], p[1], p[2]], Material::CoinEdge, 0.8);
        triangle(mesh, [p[0], p[2], p[3]], Material::CoinEdge, 0.8);
        for sign in [-1.0, 1.0] {
            let p = [
                point(0.23, a, 0.027 * sign),
                point(0.23, b, 0.027 * sign),
                point(0.195, b, 0.042 * sign),
                point(0.195, a, 0.042 * sign),
            ];
            triangle(mesh, [p[0], p[1], p[2]], Material::CoinGold, 1.05);
            triangle(mesh, [p[0], p[2], p[3]], Material::CoinGold, 1.05);
            triangle(
                mesh,
                [point(0.0, 0.0, 0.042 * sign), p[3], p[2]],
                Material::CoinFace,
                0.8,
            );
            let p = [
                point(0.167, a, 0.044 * sign),
                point(0.167, b, 0.044 * sign),
                point(0.153, b, 0.045 * sign),
                point(0.153, a, 0.045 * sign),
            ];
            triangle(mesh, [p[0], p[1], p[2]], Material::CoinGold, 0.95);
            triangle(mesh, [p[0], p[2], p[3]], Material::CoinGold, 0.95);
        }
    }
    // A small raised diamond reads clearly even in the terminal's low-resolution view.
    for sign in [-1.0, 1.0] {
        let front = normal * (0.052 * sign * scale);
        let c = at + front;
        let p = [
            c + up * (0.112 * scale),
            c + right * (0.075 * scale),
            c + up * (-0.112 * scale),
            c + right * (-0.075 * scale),
        ];
        triangle(
            mesh,
            [c + normal * (0.012 * sign * scale), p[0], p[1]],
            Material::CoinGold,
            1.15,
        );
        triangle(
            mesh,
            [c + normal * (0.012 * sign * scale), p[1], p[2]],
            Material::CoinGold,
            0.75,
        );
        triangle(
            mesh,
            [c + normal * (0.012 * sign * scale), p[2], p[3]],
            Material::CoinGold,
            0.85,
        );
        triangle(
            mesh,
            [c + normal * (0.012 * sign * scale), p[3], p[0]],
            Material::CoinGold,
            1.05,
        );
    }
}
fn sparkle(mesh: &mut Mesh, at: Vec3, size: f32, light: f32) {
    for axis in [Vec3::new(size, 0.0, 0.0), Vec3::new(0.0, 0.0, size)] {
        let up = Vec3::new(0.0, size * 2.2, 0.0);
        triangle(
            mesh,
            [at + up, at + axis, at + up * -1.0],
            Material::CoinSpark,
            light,
        );
        triangle(
            mesh,
            [at + up, at + up * -1.0, at + axis * -1.0],
            Material::CoinSpark,
            light,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_spaced_reachable_objectives() {
        for seed in [7, 42, 104729, 0xfeedbeef] {
            let map = Map::procedural(seed).unwrap();
            let cells = positions(&map);
            assert_eq!(cells, positions(&map));
            assert_eq!(cells.len(), COUNT);
            for (i, cell) in cells.iter().enumerate() {
                assert!(!map.wall(cell[0] as i32, cell[1] as i32));
                assert_ne!(*cell, map.exit);
                assert!(!cells[..i].contains(cell));
                let d = distances(&map, cell[1] * map.width() + cell[0]);
                assert!(d[map.exit[1] * map.width() + map.exit[0]] >= 8);
                assert!(d[map.exit[1] * map.width() + map.exit[0]] < u32::MAX);
            }
        }
    }
    #[test]
    fn pickup_is_once_and_does_not_cross_cell_boundary() {
        let map = Map::generated(17, 42).unwrap();
        let mut coins = Coins::new(&map);
        let c = coins.cells[0];
        let p = Vec3::new(
            c[0] as f32 * map.tile_size,
            map.eye_height,
            (c[1] as f32 + 0.5) * map.tile_size,
        );
        assert!(!coins.tick(&map, p + Vec3::new(-0.001, 0.0, 0.0), 0.02, true));
        assert!(!coins.tick(&map, p, 0.02, false));
        assert!(coins.tick(&map, p, 0.02, true));
        assert!(!coins.tick(&map, p, 0.02, true));
        assert_eq!(coins.collected(), 1);
    }
}
