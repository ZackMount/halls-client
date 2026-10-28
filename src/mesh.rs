#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    math::{Vec2, Vec3},
    world::Map,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    Wall,
    Floor,
    Ceiling,
    Light,
    Exit,
    Panel,
    Green,
    Wire,
    Rage,
    CoinGold,
    CoinEdge,
    CoinFace,
    CoinSpark,
}
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub p: Vec3,
    pub uv: Vec2,
    pub light: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub vertices: [Vertex; 3],
    pub normal: Vec3,
    pub material: Material,
    pub lightmap: u32,
}
#[derive(Default)]
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub lightmaps: Vec<Lightmap>,
    pub floor_light: Vec<u32>,
    pub light_width: usize,
}

// Quarter-metre samples and 11 fractional bits, generated entirely in memory.
pub const LIGHT_SIZE: usize = 9;
const LIGHT_STEPS: usize = LIGHT_SIZE - 1;
#[derive(Clone, Debug)]
pub struct Lightmap {
    pub origin: Vec2,
    pub scale: Vec2,
    pub samples: [u16; LIGHT_SIZE * LIGHT_SIZE],
}
impl Lightmap {
    pub fn sample(&self, uv: Vec2) -> f32 {
        let x = ((uv.x - self.origin.x) * self.scale.x).clamp(0.0, LIGHT_STEPS as u32 as f32);
        let y = ((uv.y - self.origin.y) * self.scale.y).clamp(0.0, LIGHT_STEPS as u32 as f32);
        let ix = (x as u32 as usize).min(LIGHT_STEPS - 1);
        let iy = (y as u32 as usize).min(LIGHT_STEPS - 1);
        let x = x - ix as u32 as f32;
        let y = y - iy as u32 as f32;
        let at = iy * LIGHT_SIZE + ix;
        let a = self.samples[at] as f32;
        let b = self.samples[at + 1] as f32;
        let c = self.samples[at + LIGHT_SIZE] as f32;
        let d = self.samples[at + LIGHT_SIZE + 1] as f32;
        ((a + (b - a) * x) * (1.0 - y) + (c + (d - c) * x) * y) / 2048.0
    }
}
// Exact grid traversal is deliberately separate from entity sight/game rules.
pub fn lighting_visible(map: &Map, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let s = map.tile_size;
    // A lightmap sample touching a solid boundary must not choose the
    // neighbouring room just because the ray's first epsilon points into it.
    let ox = (a.x / s).floor() as i32;
    let oz = (a.z / s).floor() as i32;
    let edge_x = a.x - ox as f32 * s < 0.0001;
    let edge_z = a.z - oz as f32 * s < 0.0001;
    if map.wall(ox, oz)
        || edge_x && map.wall(ox - 1, oz)
        || edge_z && map.wall(ox, oz - 1)
        || edge_x && edge_z && map.wall(ox - 1, oz - 1)
    {
        return false;
    }
    let mut x = ((a.x + d.x * 0.00001) / s).floor() as i32;
    let mut z = ((a.z + d.z * 0.00001) / s).floor() as i32;
    let sx = if d.x > 0.0 { 1 } else { -1 };
    let sz = if d.z > 0.0 { 1 } else { -1 };
    let tx = if d.x.abs() < 0.000001 {
        f32::INFINITY
    } else {
        s / d.x.abs()
    };
    let tz = if d.z.abs() < 0.000001 {
        f32::INFINITY
    } else {
        s / d.z.abs()
    };
    let mut nx = if tx.is_infinite() {
        tx
    } else {
        ((x + if sx > 0 { 1 } else { 0 }) as f32 * s - a.x) / d.x
    };
    let mut nz = if tz.is_infinite() {
        tz
    } else {
        ((z + if sz > 0 { 1 } else { 0 }) as f32 * s - a.z) / d.z
    };
    loop {
        if map.wall(x, z) {
            return false;
        }
        if nx.min(nz) >= 0.99999 {
            return true;
        }
        if (nx - nz).abs() < 0.000001 {
            if map.wall(x + sx, z) || map.wall(x, z + sz) {
                return false;
            }
            x += sx;
            z += sz;
            nx += tx;
            nz += tz;
        } else if nx < nz {
            x += sx;
            nx += tx;
        } else {
            z += sz;
            nz += tz;
        }
    }
}

fn ambient(n: Vec3) -> f32 {
    0.001 + 0.001 * (n.y * 0.5 + 0.5)
}
fn lamp_light(p: Vec3, n: Vec3, lamp: Vec3) -> f32 {
    let delta = lamp - p;
    let d2 = delta.dot(delta);
    if d2 >= 49.0 {
        return 0.0;
    }
    let incidence = (n.dot(delta) / d2.sqrt()).max(0.0);
    let fade = 1.0 - d2 / 49.0;
    2.7 * (0.12 + 0.88 * incidence) * fade * fade / (1.0 + d2 * 0.35)
}

struct Builder<'a> {
    map: &'a Map,
    lights: Vec<Option<Vec3>>,
    lightmaps: Vec<Lightmap>,
    floor_light: Vec<u32>,
    triangles: Vec<Triangle>,
    visibility: Vec<u8>,
    visibility_bounds: [f32; 4],
}
impl Builder<'_> {
    fn nearby(&self, min: Vec3, max: Vec3) -> Vec<Vec3> {
        let s = self.map.tile_size;
        let mut lamps = vec![];
        for z in (((min.z - 7.0) / s).floor() as i32).max(0)
            ..=(((max.z + 7.0) / s).floor() as i32).min(self.map.height() as i32 - 1)
        {
            for x in (((min.x - 7.0) / s).floor() as i32).max(0)
                ..=(((max.x + 7.0) / s).floor() as i32).min(self.map.width() as i32 - 1)
            {
                if let Some(lamp) = self.lights[z as usize * self.map.width() + x as usize] {
                    lamps.push(lamp);
                }
            }
        }
        lamps
    }
    fn illumination(&self, p: Vec3, n: Vec3) -> f32 {
        let p = p + n * 0.025;
        let mut light = ambient(n);
        for lamp in self.nearby(p, p) {
            if (lamp - p).dot(lamp - p) < 49.0 && lighting_visible(self.map, p, lamp) {
                light += lamp_light(p, n, lamp);
            }
        }
        light.min(1.6)
    }
    fn bake(&mut self, p: [Vec3; 4], uv: [Vec2; 4], n: Vec3) -> Lightmap {
        let steps = LIGHT_STEPS as u32 as f32;
        let scale = |d: f32| if d.abs() < 0.0001 { 0.0 } else { steps / d };
        let swap = uv[1].x == uv[0].x;
        let points = core::array::from_fn::<_, { LIGHT_SIZE * LIGHT_SIZE }, _>(|i| {
            let (x, y) = (
                (i % LIGHT_SIZE) as f32 / steps,
                (i / LIGHT_SIZE) as f32 / steps,
            );
            let (u, v) = if swap { (y, x) } else { (x, y) };
            p[0].lerp(p[1], u).lerp(p[3].lerp(p[2], u), v) + n * 0.025
        });
        let min = Vec3::new(
            p.iter().map(|v| v.x).fold(f32::INFINITY, f32::min),
            0.0,
            p.iter().map(|v| v.z).fold(f32::INFINITY, f32::min),
        );
        let max = Vec3::new(
            p.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max),
            0.0,
            p.iter().map(|v| v.z).fold(f32::NEG_INFINITY, f32::max),
        );
        let vertical = n.y == 0.0;
        let mut values = [ambient(n); LIGHT_SIZE * LIGHT_SIZE];
        // Visibility is two-dimensional: reuse each wall-column ray at every height.
        // The nearby lamp list is also shared by the entire surface.
        let lamps = self.nearby(min, max);
        let horizontal = n.x == 0.0 && n.z == 0.0;
        let bounds = [min.x, min.z, max.x, max.z];
        let reuse = horizontal && self.visibility_bounds == bounds;
        if horizontal && !reuse {
            self.visibility.resize(lamps.len() * values.len(), 0);
            self.visibility_bounds = bounds;
        }
        for (l, lamp) in lamps.into_iter().enumerate() {
            for i in 0..if vertical { LIGHT_SIZE } else { values.len() } {
                let at = points[i];
                let index = l * values.len() + i;
                let visible = if reuse {
                    self.visibility[index] != 0
                } else {
                    (lamp.x - at.x).powi(2) + (lamp.z - at.z).powi(2) < 49.0
                        && lighting_visible(self.map, at, lamp)
                };
                if horizontal && !reuse {
                    self.visibility[index] = u8::from(visible);
                }
                if !visible {
                    continue;
                }
                for row in 0..if vertical { LIGHT_SIZE } else { 1 } {
                    let j = i + row * LIGHT_SIZE;
                    values[j] += lamp_light(points[j], n, lamp);
                }
            }
        }
        Lightmap {
            origin: uv[0],
            scale: Vec2::new(scale(uv[2].x - uv[0].x), scale(uv[2].y - uv[0].y)),
            samples: values.map(|v| (v.min(1.6) * 2048.0 + 0.5) as u16),
        }
    }
    fn quad(&mut self, p: [Vec3; 4], uv: [Vec2; 4], material: Material) {
        let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalized();
        let baked = matches!(
            material,
            Material::Wall | Material::Floor | Material::Ceiling
        );
        let v = core::array::from_fn::<_, 4, _>(|i| Vertex {
            p: p[i],
            uv: uv[i],
            light: if baked
                || matches!(material, Material::Light | Material::Exit | Material::Green)
            {
                1.0
            } else {
                self.illumination(p[i], normal)
            },
        });
        let index = if baked {
            let patch = self.bake(p, uv, normal);
            let index = self.lightmaps.len() as u32;
            if material == Material::Floor {
                let x = (p[0].x / self.map.tile_size) as usize;
                let z = (p[0].z / self.map.tile_size) as usize;
                self.floor_light[z * self.map.width() + x] = index;
            }
            self.lightmaps.push(patch);
            index
        } else {
            u32::MAX
        };
        append_quad(&mut self.triangles, v, normal, material, index);
    }
    fn wall(&mut self, p: [Vec3; 4], material: Material) {
        // Shared world positions must also share wallpaper and stain coordinates.
        let uv = p.map(|v| Vec2::new(v.x + v.z, v.y));
        self.quad(p, uv, material);
    }
    #[cfg(any(feature = "tools", test))]
    fn box_mesh(&mut self, min: Vec3, max: Vec3, material: Material) {
        for (face, corners) in [
            [0, 4, 6, 2],
            [5, 1, 3, 7],
            [1, 0, 2, 3],
            [4, 5, 7, 6],
            [2, 6, 7, 3],
            [0, 1, 5, 4],
        ]
        .into_iter()
        .enumerate()
        {
            let p = corners.map(|i| {
                Vec3::new(
                    if i & 1 == 0 { min.x } else { max.x },
                    if i & 2 == 0 { min.y } else { max.y },
                    if i & 4 == 0 { min.z } else { max.z },
                )
            });
            if face < 4 {
                self.wall(p, material);
            } else {
                self.quad(p, [Vec2::default(); 4], material);
            }
        }
    }
}

impl Mesh {
    pub fn probe(&self, p: Vec3) -> f32 {
        if self.floor_light.is_empty() {
            return 1.0;
        }
        let x = (p.x / 2.0).floor() as i32;
        let z = (p.z / 2.0).floor() as i32;
        if x < 0 || z < 0 || x as usize >= self.light_width || z as usize >= self.light_width {
            return 0.035;
        }
        self.floor_light
            .get(z as usize * self.light_width + x as usize)
            .and_then(|&i| self.lightmaps.get(i as usize))
            .map_or(0.035, |map| map.sample(Vec2::new(p.x, p.z)))
    }
    pub fn add_quad(&mut self, p: [Vec3; 4], material: Material) {
        let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalized();
        let vertices = p.map(|p| Vertex {
            p,
            uv: Vec2::default(),
            light: if material == Material::Wire {
                self.probe(p)
            } else {
                1.0
            },
        });
        append_quad(&mut self.triangles, vertices, normal, material, u32::MAX);
    }
    pub fn from_map(map: &Map) -> Self {
        let s = map.tile_size;
        let c = map.ceiling;
        let layout = crate::lighting::layout(map);
        let mut lights = vec![None; map.width() * map.height()];
        let mut fixtures = vec![];
        for z in 0..map.height() {
            for x in 0..map.width() {
                if layout[z * map.width() + x] != 0 {
                    let lamp = Vec3::new(
                        (x as u32 as f32 + 0.5) * s,
                        c - 0.15,
                        (z as u32 as f32 + 0.5) * s,
                    );
                    let active = layout[z * map.width() + x] == 2;
                    fixtures.push((lamp, active));
                    if active {
                        lights[z * map.width() + x] = Some(lamp);
                    }
                }
            }
        }
        let mut b = Builder {
            map,
            lights,
            lightmaps: vec![],
            floor_light: vec![u32::MAX; map.width() * map.height()],
            triangles: vec![],
            visibility: vec![],
            visibility_bounds: [f32::NAN; 4],
        };
        for z in 0..map.height() {
            for x in 0..map.width() {
                if map.wall(x as i32, z as i32) {
                    continue;
                }
                let (x0, z0) = (x as u32 as f32 * s, z as u32 as f32 * s);
                let (x1, z1) = (x0 + s, z0 + s);
                let (h00, h10, h01, h11) = (
                    map.cell_floor(x, z, 0.0, 0.0),
                    map.cell_floor(x, z, 1.0, 0.0),
                    map.cell_floor(x, z, 0.0, 1.0),
                    map.cell_floor(x, z, 1.0, 1.0),
                );
                b.quad(
                    [
                        Vec3::new(x0, h00, z0),
                        Vec3::new(x0, h01, z1),
                        Vec3::new(x1, h11, z1),
                        Vec3::new(x1, h10, z0),
                    ],
                    [
                        Vec2::new(x0, z0),
                        Vec2::new(x0, z1),
                        Vec2::new(x1, z1),
                        Vec2::new(x1, z0),
                    ],
                    if [x, z] == map.exit {
                        Material::Green
                    } else {
                        Material::Floor
                    },
                );
                b.quad(
                    [
                        Vec3::new(x0, c, z0),
                        Vec3::new(x1, c, z0),
                        Vec3::new(x1, c, z1),
                        Vec3::new(x0, c, z1),
                    ],
                    [
                        Vec2::new(x0, z0),
                        Vec2::new(x1, z0),
                        Vec2::new(x1, z1),
                        Vec2::new(x0, z1),
                    ],
                    Material::Ceiling,
                );
                let boundaries = [
                    (
                        x as i32,
                        z as i32 - 1,
                        [
                            Vec3::new(x0, h00, z0),
                            Vec3::new(x1, h10, z0),
                            Vec3::new(x1, c, z0),
                            Vec3::new(x0, c, z0),
                        ],
                    ),
                    (
                        x as i32 + 1,
                        z as i32,
                        [
                            Vec3::new(x1, h10, z0),
                            Vec3::new(x1, h11, z1),
                            Vec3::new(x1, c, z1),
                            Vec3::new(x1, c, z0),
                        ],
                    ),
                    (
                        x as i32,
                        z as i32 + 1,
                        [
                            Vec3::new(x1, h11, z1),
                            Vec3::new(x0, h01, z1),
                            Vec3::new(x0, c, z1),
                            Vec3::new(x1, c, z1),
                        ],
                    ),
                    (
                        x as i32 - 1,
                        z as i32,
                        [
                            Vec3::new(x0, h01, z1),
                            Vec3::new(x0, h00, z0),
                            Vec3::new(x0, c, z0),
                            Vec3::new(x0, c, z1),
                        ],
                    ),
                ];
                for (nx, nz, p) in boundaries {
                    if map.wall(nx, nz) {
                        b.wall(p, Material::Wall);
                    } else {
                        // An exposed riser between different floor heights is actual geometry.
                        let at = |point: Vec3| {
                            map.cell_floor(
                                nx as usize,
                                nz as usize,
                                (point.x / s - nx as f32).clamp(0.0, 1.0),
                                (point.z / s - nz as f32).clamp(0.0, 1.0),
                            )
                        };
                        let (a, d) = (at(p[0]), at(p[1]));
                        if a > p[0].y + 0.001 || d > p[1].y + 0.001 {
                            b.wall(
                                [
                                    p[0],
                                    p[1],
                                    Vec3::new(p[1].x, d.max(p[1].y), p[1].z),
                                    Vec3::new(p[0].x, a.max(p[0].y), p[0].z),
                                ],
                                Material::Wall,
                            );
                        }
                    }
                }
            }
        }
        for (lamp, active) in fixtures {
            let broad = 0.27;
            b.quad(
                [
                    Vec3::new(lamp.x - 0.55, c - 0.015, lamp.z - broad),
                    Vec3::new(lamp.x + 0.55, c - 0.015, lamp.z - broad),
                    Vec3::new(lamp.x + 0.55, c - 0.015, lamp.z + broad),
                    Vec3::new(lamp.x - 0.55, c - 0.015, lamp.z + broad),
                ],
                [Vec2::default(); 4],
                if active {
                    Material::Light
                } else {
                    Material::Panel
                },
            );
        }
        #[cfg(any(feature = "tools", test))]
        for block in &map.blocks {
            b.box_mesh(
                Vec3::new(block.min[0], block.min[1], block.min[2]),
                Vec3::new(block.max[0], block.max[1], block.max[2]),
                Material::Wall,
            );
        }

        Self {
            triangles: b.triangles,
            lightmaps: b.lightmaps,
            floor_light: b.floor_light,
            light_width: map.width(),
        }
    }
}

fn append_quad(
    out: &mut Vec<Triangle>,
    v: [Vertex; 4],
    normal: Vec3,
    material: Material,
    lightmap: u32,
) {
    out.extend_from_slice(&[
        Triangle {
            vertices: [v[0], v[1], v[2]],
            normal,
            material,
            lightmap,
        },
        Triangle {
            vertices: [v[0], v[2], v[3]],
            normal,
            material,
            lightmap,
        },
    ]);
}

#[cfg(test)]
mod lighting_tests {
    use super::*;
    use crate::lighting::grid_active as lamp_active;
    fn map_with(solid: impl Fn(usize, usize) -> bool) -> Map {
        let mut map = Map::generated(7, 1).unwrap();
        for z in 0..7 {
            for x in 0..7 {
                map.set_wall(x, z, solid(x, z));
            }
        }
        map
    }
    #[test]
    fn fewer_active_lamps_keep_the_original_fixture_grid() {
        let spawn = [127, 131];
        let mut original = 0;
        let mut retained = 0;
        for seed in (0..32).chain([104729, 220033, u64::MAX, 1 << 40]) {
            let mut old_count = 0;
            let mut new_count = 0;
            for z in 0..256 {
                for x in 0..256 {
                    if (x + 2 * z) % 4 != 0 && [x, z] != spawn {
                        continue;
                    }
                    let old =
                        [x, z] == spawn || !((x * 37 + z * 19) as u64 ^ seed).is_multiple_of(7);
                    let active = lamp_active(x, z, seed, spawn);
                    assert!(!active || old, "an originally dark fixture was enabled");
                    assert_eq!(active, lamp_active(x, z, seed, spawn));
                    old_count += usize::from(old);
                    new_count += usize::from(active);
                }
            }
            assert!(lamp_active(spawn[0], spawn[1], seed, spawn));
            let retention = new_count as f64 / old_count as f64;
            assert!(
                (0.68..0.72).contains(&retention),
                "seed {seed}: {retention}"
            );
            original += old_count;
            retained += new_count;
        }
        let retention = retained as f64 / original as f64;
        assert!((0.697..0.703).contains(&retention), "{retention}");

        let map = map_with(|_, _| false);
        let spawn = [
            (map.spawn.x / map.tile_size) as usize,
            (map.spawn.z / map.tile_size) as usize,
        ];
        let fixture_count = (0..7)
            .flat_map(|z| (0..7).map(move |x| (x, z)))
            .filter(|&(x, z)| (x + 2 * z) % 4 == 0 || [x, z] == spawn)
            .count();
        let mesh = Mesh::from_map(&map);
        assert_eq!(
            mesh.triangles
                .iter()
                .filter(|t| matches!(t.material, Material::Light | Material::Panel))
                .count(),
            fixture_count * 2
        );
    }
    #[test]
    fn client_lamps_match_the_replay_bitmap_contract() {
        for (seed, expected) in [
            (0, 0x3bafd101),
            (42, 0xc858a82c),
            (104729, 0xd5cb2412),
            (1 << 40, 0x0757925d),
            (u64::MAX, 0x412c2d0e),
        ] {
            let mut hash = 2166136261u32;
            for z in 0..256 {
                for byte in 0..32 {
                    let mut bits = 0;
                    for bit in 0..8 {
                        bits |= u8::from(lamp_active(byte * 8 + bit, z, seed, [127, 131])) << bit;
                    }
                    hash = (hash ^ u32::from(bits)).wrapping_mul(16777619);
                }
            }
            assert_eq!(hash, expected, "seed {seed}");
        }
    }
    #[test]
    fn lighting_does_not_skip_grazing_walls_or_diagonal_cracks() {
        let map = map_with(|x, z| x == 2 && z == 2);
        assert!(!lighting_visible(
            &map,
            Vec3::new(3.99, 1.0, 4.001),
            Vec3::new(5.0, 1.0, 3.98)
        ));
        let map = map_with(|x, z| x == 2 && z == 1);
        assert!(!lighting_visible(
            &map,
            Vec3::new(3.0, 1.0, 3.0),
            Vec3::new(5.0, 1.0, 5.0)
        ));
    }
    #[test]
    fn a_sealed_room_has_no_direct_light_and_shared_edges_match() {
        let map = map_with(|x, _| x == 3);
        let mut lamps = vec![None; 49];
        lamps[2 * 7 + 1] = Some(Vec3::new(3.0, 2.5, 5.0));
        let mut b = Builder {
            map: &map,
            lights: lamps,
            lightmaps: vec![],
            floor_light: vec![u32::MAX; 49],
            triangles: vec![],
            visibility: vec![],
            visibility_bounds: [f32::NAN; 4],
        };
        let up = Vec3::new(0.0, 1.0, 0.0);
        assert!((b.illumination(Vec3::new(9.0, 0.0, 5.0), up) - 0.002).abs() < 0.00001);
        assert!(b.illumination(Vec3::new(3.0, 0.0, 5.0), up) > 0.2);
        for x in [0.0, 2.0] {
            b.quad(
                [
                    Vec3::new(x, 0.0, 2.0),
                    Vec3::new(x, 0.0, 4.0),
                    Vec3::new(x + 2.0, 0.0, 4.0),
                    Vec3::new(x + 2.0, 0.0, 2.0),
                ],
                [
                    Vec2::new(x, 2.0),
                    Vec2::new(x, 4.0),
                    Vec2::new(x + 2.0, 4.0),
                    Vec2::new(x + 2.0, 2.0),
                ],
                Material::Floor,
            );
        }
        for i in 0..21 {
            let uv = Vec2::new(2.0, 2.0 + i as f32 * 0.1);
            assert!((b.lightmaps[0].sample(uv) - b.lightmaps[1].sample(uv)).abs() < 0.00001);
        }
        assert_eq!(b.triangles.len(), 4);
    }
    #[test]
    fn boundary_samples_do_not_borrow_light_from_the_other_room() {
        let map = map_with(|x, z| (x == 1 && z == 2) || (x == 2 && z == 1));
        assert!(!lighting_visible(
            &map,
            Vec3::new(4.0, 0.025, 4.0),
            Vec3::new(3.0, 2.5, 3.0)
        ));
        assert!(!lighting_visible(
            &map,
            Vec3::new(4.0, 2.625, 4.0),
            Vec3::new(3.0, 2.5, 3.0)
        ));
        assert!(lighting_visible(
            &map,
            Vec3::new(5.0, 0.025, 5.0),
            Vec3::new(5.0, 2.5, 5.0)
        ));
    }
    #[test]
    fn reused_floor_ceiling_visibility_matches_independent_rays() {
        let map = map_with(|x, z| x == 3 && z >= 2);
        let mut lamps = vec![None; 49];
        lamps[7 + 1] = Some(Vec3::new(3.0, 2.5, 3.0));
        lamps[3 * 7 + 5] = Some(Vec3::new(11.0, 2.5, 7.0));
        let mut b = Builder {
            map: &map,
            lights: lamps,
            lightmaps: vec![],
            floor_light: vec![u32::MAX; 49],
            triangles: vec![],
            visibility: vec![],
            visibility_bounds: [f32::NAN; 4],
        };
        for x in [2.0, 8.0] {
            for (height, sign) in [(0.0, 1.0), (2.65, -1.0)] {
                let p = [
                    Vec3::new(x, height, 4.0),
                    Vec3::new(x, height, 6.0),
                    Vec3::new(x + 2.0, height, 6.0),
                    Vec3::new(x + 2.0, height, 4.0),
                ];
                let uv = p.map(|v| Vec2::new(v.x, v.z));
                let normal = Vec3::new(0.0, sign, 0.0);
                let patch = b.bake(p, uv, normal);
                for y in 0..LIGHT_SIZE {
                    for i in 0..LIGHT_SIZE {
                        let at = Vec3::new(x + i as f32 * 0.25, height, 4.0 + y as f32 * 0.25);
                        let direct = (b.illumination(at, normal) * 2048.0 + 0.5) as u16;
                        assert_eq!(
                            patch.samples[y * LIGHT_SIZE + i],
                            direct,
                            "cached shadow differs at {at:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn wall_material_and_light_are_continuous_in_all_four_directions() {
        let map = map_with(|_, _| false);
        let mut lamps = vec![None; 49];
        lamps[3 * 7 + 3] = Some(Vec3::new(7.0, 2.5, 7.0));
        for (start, step) in [
            ((2.0, 2.0), (2.0, 0.0)),
            ((12.0, 2.0), (0.0, 2.0)),
            ((12.0, 12.0), (-2.0, 0.0)),
            ((2.0, 12.0), (0.0, -2.0)),
        ] {
            let mut b = Builder {
                map: &map,
                lights: lamps.clone(),
                lightmaps: vec![],
                floor_light: vec![u32::MAX; 49],
                triangles: vec![],
                visibility: vec![],
                visibility_bounds: [f32::NAN; 4],
            };
            for segment in 0..2 {
                let a = Vec3::new(
                    start.0 + step.0 * segment as f32,
                    0.0,
                    start.1 + step.1 * segment as f32,
                );
                let c = a + Vec3::new(step.0, 0.0, step.1);
                b.wall(
                    [
                        a,
                        c,
                        c + Vec3::new(0.0, 2.65, 0.0),
                        a + Vec3::new(0.0, 2.65, 0.0),
                    ],
                    Material::Wall,
                );
            }
            let left = b.triangles[0];
            let right = b.triangles[2];
            assert_eq!(
                left.vertices[1].uv.x, right.vertices[0].uv.x,
                "wallpaper must not restart at a segment boundary: {step:?}"
            );
            let edge_u = left.vertices[1].p.x + left.vertices[1].p.z;
            for i in 0..41 {
                let uv = Vec2::new(edge_u, 2.65 * i as f32 / 40.0);
                assert!(
                    (b.lightmaps[0].sample(uv) - b.lightmaps[1].sample(uv)).abs() < 0.00001,
                    "light must agree all along the shared edge: {step:?}"
                );
            }
            // Signed texture axes must still map to every baked sample,
            // not clamp a reversed face to its first column.
            for (index, t) in [left, right].iter().enumerate() {
                for y in 0..LIGHT_SIZE {
                    for x in 0..LIGHT_SIZE {
                        let u = x as f32 / LIGHT_STEPS as f32;
                        let v = y as f32 / LIGHT_STEPS as f32;
                        let p = t.vertices[0].p.lerp(t.vertices[1].p, u)
                            + Vec3::new(0.0, 2.65 * v, 0.0);
                        let uv = Vec2::new(p.x + p.z, p.y);
                        let expected =
                            (b.illumination(p, t.normal) * 2048.0 + 0.5) as u16 as f32 / 2048.0;
                        assert!((b.lightmaps[index].sample(uv) - expected).abs() < 0.00001);
                    }
                }
            }
        }
    }
    #[test]
    fn the_black_stripe_shares_the_wall_surface() {
        let map = map_with(|_, _| false);
        let mut b = Builder {
            map: &map,
            lights: vec![None; 49],
            lightmaps: vec![],
            floor_light: vec![u32::MAX; 49],
            triangles: vec![],
            visibility: vec![],
            visibility_bounds: [f32::NAN; 4],
        };
        b.wall(
            [
                Vec3::new(2.0, 0.0, 2.0),
                Vec3::new(4.0, 0.0, 2.0),
                Vec3::new(4.0, 2.65, 2.0),
                Vec3::new(2.0, 2.65, 2.0),
            ],
            Material::Wall,
        );
        assert_eq!(b.triangles.len(), 2);
        assert!(
            b.triangles
                .iter()
                .flat_map(|t| t.vertices)
                .all(|v| v.p.z == 2.0)
        );
    }
}
