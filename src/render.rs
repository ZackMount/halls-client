#[cfg(feature = "tools")]
use crate::error::Result;
#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    math::{Camera, Vec2, Vec3},
    mesh::{Material, Mesh},
};
#[cfg(feature = "tools")]
use std::{io::Write, path::Path};

pub const MAX_RENDER_SCALE: usize = 64;
const FOG_COLOR: [f32; 3] = [0.032, 0.032, 0.026];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);
#[derive(Clone)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgb>,
    pub depth: Vec<f32>,
}
impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![encode_color(FOG_COLOR, 0, 0, true, 0.0); width * height],
            depth: vec![f32::INFINITY; width * height],
        }
    }
    #[cfg(feature = "tools")]
    pub fn ppm(&self, path: &Path) -> Result<()> {
        let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
        writeln!(out, "P6\n{} {}\n255", self.width, self.height)?;
        for Rgb(r, g, b) in &self.pixels {
            out.write_all(&[*r, *g, *b])?;
        }
        out.flush()?;
        Ok(())
    }
    pub fn downsample(self, factor: usize) -> Self {
        if factor == 1 {
            return self;
        }
        let mut out = Self::new(self.width / factor, self.height / factor);
        for y in 0..out.height {
            for x in 0..out.width {
                let mut sum = [0usize; 3];
                let mut depth = f32::INFINITY;
                for dy in 0..factor {
                    for dx in 0..factor {
                        let i = (y * factor + dy) * self.width + x * factor + dx;
                        let Rgb(r, g, b) = self.pixels[i];
                        sum[0] += r as usize;
                        sum[1] += g as usize;
                        sum[2] += b as usize;
                        depth = depth.min(self.depth[i]);
                    }
                }
                let n = factor * factor;
                out.pixels[y * out.width + x] =
                    Rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8);
                out.depth[y * out.width + x] = depth;
            }
        }
        out
    }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct ClipVertex {
    p: Vec3,
    uv: Vec2,
    light: f32,
}
impl ClipVertex {
    fn lerp(self, b: Self, t: f32) -> Self {
        Self {
            p: self.p.lerp(b.p, t),
            uv: self.uv * (1.0 - t) + b.uv * t,
            light: self.light * (1.0 - t) + b.light * t,
        }
    }
}
#[derive(Clone, Copy)]
struct ScreenVertex {
    x: f32,
    y: f32,
    inv_z: f32,
    uv_over_z: Vec2,
    light_over_z: f32,
}
#[cfg(feature = "tools")]
#[derive(Clone, Copy, Default)]
pub struct RenderStats {
    pub submitted: usize,
    pub visible: usize,
    pub fragments: usize,
}
macro_rules! setting {
    ($renderer:expr,$field:ident,$default:expr) => {{
        #[cfg(any(feature = "tools", test))]
        {
            $renderer.$field
        }
        #[cfg(not(any(feature = "tools", test)))]
        {
            $default
        }
    }};
}
pub struct Renderer {
    #[cfg(any(feature = "tools", test))]
    pub near: f32,
    #[cfg(any(feature = "tools", test))]
    pub far: f32,
    #[cfg(any(feature = "tools", test))]
    pub exposure: f32,
    #[cfg(any(feature = "tools", test))]
    pub pixel_aspect: f32,
    #[cfg(any(feature = "tools", test))]
    pub fog: bool,
    #[cfg(feature = "tools")]
    pub stats: RenderStats,
    pub flashlight: bool,
    #[cfg(any(feature = "tools", test))]
    pub retro: bool,
}
impl Default for Renderer {
    fn default() -> Self {
        Self {
            #[cfg(any(feature = "tools", test))]
            near: 0.065,
            #[cfg(any(feature = "tools", test))]
            far: 96.0,
            #[cfg(any(feature = "tools", test))]
            exposure: 1.05,
            #[cfg(any(feature = "tools", test))]
            pixel_aspect: 1.0,
            #[cfg(any(feature = "tools", test))]
            fog: true,
            #[cfg(feature = "tools")]
            stats: RenderStats::default(),
            flashlight: false,
            #[cfg(any(feature = "tools", test))]
            retro: true,
        }
    }
}

// A triangle clipped by six convex planes has at most nine vertices.
// Twelve slots leave numerical headroom, with bounds checks still enabled.
const CLIP_CAPACITY: usize = 12;
fn clip_polygon(
    input: &[ClipVertex],
    normal: Vec3,
    d: f32,
    output: &mut [ClipVertex; CLIP_CAPACITY],
) -> usize {
    let mut len = 0;
    if input.is_empty() {
        return 0;
    }
    let mut prev = *input.last().unwrap();
    let mut pd = normal.dot(prev.p) + d;
    for &cur in input {
        let cd = normal.dot(cur.p) + d;
        if (pd >= 0.0) != (cd >= 0.0) {
            output[len] = prev.lerp(cur, pd / (pd - cd));
            len += 1;
        }
        if cd >= 0.0 {
            output[len] = cur;
            len += 1;
        }
        prev = cur;
        pd = cd;
    }
    len
}
fn edge(a: ScreenVertex, b: ScreenVertex, x: f32, y: f32) -> f32 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}
fn grain(u: f32, v: f32) -> f32 {
    let mut n = (u * 39.0).floor() as i32 as u32;
    n = n.wrapping_mul(0x45d9f3b) ^ (v * 39.0).floor() as i32 as u32;
    n ^= n >> 16;
    n = n.wrapping_mul(0x45d9f3b);
    (n & 255) as f32 / 255.0
}
fn texture(material: Material, uv: Vec2, footprint: f32) -> [f32; 3] {
    let n = (grain(uv.x, uv.y) - 0.5) * 0.035;
    match material {
        Material::Wall if uv.y < 0.12 - footprint * 0.5 => [0.025, 0.029, 0.031],
        Material::Wall => {
            let u = (uv.x * 5.0).rem_euclid(1.0) - 0.5;
            let v = (uv.y * 5.0).rem_euclid(1.0) - 0.5;
            let print = (u.abs() + v.abs() - 0.31).abs() < 0.055;
            let stain = (uv.x * 0.61 + 0.3).sin() * (uv.y * 1.9).sin();
            let base = if print { 0.76 } else { 1.0 };
            let dirt = 1.0 - 0.22 * stain.max(0.0) - 0.17 * (1.0 - uv.y / 0.55).clamp(0.0, 1.0);
            let base = [
                (0.64 + n) * base * dirt,
                (0.48 + n) * base * dirt,
                (0.19 + n) * base * dirt,
            ];
            let mask = ((0.12 - uv.y) / footprint.max(0.000001) + 0.5).clamp(0.0, 1.0);
            let trim = [0.025, 0.029, 0.031];
            core::array::from_fn(|i| base[i] * (1.0 - mask) + trim[i] * mask)
        }
        Material::Floor => {
            let fibers = (grain(uv.x * 1.3, uv.y * 1.3) - 0.5) * 0.045;
            let damp = ((uv.x * 0.71).sin() * (uv.y * 0.83 + 1.0).sin()).max(0.0);
            let patch = 1.0 - 0.36 * damp;
            [
                (0.16 + fibers) * patch,
                (0.115 + fibers * 0.8) * patch,
                (0.058 + fibers * 0.5) * patch,
            ]
        }
        Material::Ceiling => {
            let seam = uv.x.rem_euclid(0.6) < 0.019 || uv.y.rem_euclid(0.6) < 0.019;
            let stain = 0.035 * ((uv.x * 1.1).sin() * (uv.y * 0.73).sin()).max(0.0);
            let m = if seam { 0.42 } else { 1.0 };
            [
                (0.43 + n - stain) * m,
                (0.40 + n - stain) * m,
                (0.29 + n - stain) * m,
            ]
        }
        Material::Light => [1.2, 1.15, 1.02],
        Material::Exit => [0.93, 0.39, 0.08],
        Material::Panel => [0.045, 0.052, 0.048],
        Material::Green => [0.045, 0.65, 0.085],
        Material::Wire => [0.009, 0.011, 0.009],
        Material::Rage => [0.0, 0.0, 0.0],
        Material::CoinGold => [0.94, 0.59, 0.12],
        Material::CoinEdge => [0.47, 0.25, 0.045],
        Material::CoinFace => [0.43, 0.235, 0.038],
        Material::CoinSpark => [1.1, 0.77, 0.23],
    }
}
fn srgb(v: f32) -> u8 {
    ((if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.max(0.0).powf(1.0 / 2.4) - 0.055
    })
    .clamp(0.0, 1.0)
        * 255.0
        + 0.5) as u8
}

fn fog_amount(distance: f32) -> f32 {
    let t = ((distance - 18.0) / 78.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn encode_color(linear: [f32; 3], x: usize, y: usize, retro: bool, detail: f32) -> Rgb {
    const BAYER: [[i16; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let color = linear.map(|v| {
        let color = srgb(v);
        if retro {
            let d = ((BAYER[y % 4][x % 4] - 8) as f32 * detail) as i16;
            (((color as i16 + d).clamp(0, 255) / 12) * 12).min(255) as u8
        } else {
            color
        }
    });
    Rgb(color[0], color[1], color[2])
}

impl Renderer {
    /// Supersample internally, retaining the terminal's character-grid dimensions.
    pub fn render_scaled(
        &mut self,
        mesh: &Mesh,
        camera: Camera,
        width: usize,
        height: usize,
        scale: usize,
    ) -> Frame {
        let scale = scale.clamp(1, MAX_RENDER_SCALE);
        self.render(mesh, camera, width * scale, height * scale)
            .downsample(scale)
    }
    pub fn render(&mut self, mesh: &Mesh, camera: Camera, width: usize, height: usize) -> Frame {
        let mut frame = Frame::new(width, height);
        self.draw(mesh, camera, &mut frame);
        frame
    }
    fn draw(&mut self, mesh: &Mesh, camera: Camera, frame: &mut Frame) {
        let (width, height) = (frame.width, frame.height);
        let fog_color = FOG_COLOR.map(|v| v * (mesh.probe(camera.position) * 2.0).clamp(0.0, 1.0));
        frame.pixels.fill(encode_color(
            fog_color,
            0,
            0,
            setting!(self, retro, true),
            0.0,
        ));
        #[cfg(feature = "tools")]
        {
            self.stats = RenderStats {
                submitted: mesh.triangles.len(),
                ..Default::default()
            };
        }
        let tan_x = (camera.fov * 0.5).tan();
        let tan_y =
            tan_x * height as u32 as f32 / width as u32 as f32 / setting!(self, pixel_aspect, 1.0);
        let planes = [
            (Vec3::new(0.0, 0.0, 1.0), -setting!(self, near, 0.065)),
            (Vec3::new(0.0, 0.0, -1.0), setting!(self, far, 96.0)),
            (Vec3::new(1.0, 0.0, tan_x), 0.0),
            (Vec3::new(-1.0, 0.0, tan_x), 0.0),
            (Vec3::new(0.0, 1.0, tan_y), 0.0),
            (Vec3::new(0.0, -1.0, tan_y), 0.0),
        ];
        let right = camera.right();
        let up = camera.up();
        let forward = camera.forward();
        let view = |p: Vec3| {
            let d = p - camera.position;
            Vec3::new(d.dot(right), d.dot(up), d.dot(forward))
        };
        let mut vertices = [ClipVertex::default(); CLIP_CAPACITY];
        let mut work = vertices;
        for tri in &mesh.triangles {
            let far = setting!(self, far, 96.0);
            let outside = [
                (false, false, camera.position.x - far),
                (false, true, camera.position.x + far),
                (true, false, camera.position.z - far),
                (true, true, camera.position.z + far),
            ]
            .iter()
            .any(|&(z, above, bound)| {
                tri.vertices.iter().all(|v| {
                    let value = if z { v.p.z } else { v.p.x };
                    if above { value > bound } else { value < bound }
                })
            });
            if outside {
                continue;
            }
            if !matches!(
                tri.material,
                Material::Rage
                    | Material::Green
                    | Material::CoinGold
                    | Material::CoinEdge
                    | Material::CoinFace
                    | Material::CoinSpark
            ) && tri.normal.dot(camera.position - tri.vertices[0].p) <= 0.0
            {
                continue;
            }
            let (mut polygon, mut scratch) = (&mut vertices, &mut work);
            for (out, v) in polygon.iter_mut().zip(tri.vertices) {
                *out = ClipVertex {
                    p: view(v.p),
                    uv: v.uv,
                    light: v.light,
                };
            }
            let mut count = 3;
            for (n, d) in planes {
                let mut inside = 0;
                for v in &polygon[..count] {
                    inside += usize::from(n.dot(v.p) + d >= 0.0);
                }
                if inside == count {
                    continue;
                }
                if inside == 0 {
                    count = 0;
                    break;
                }
                count = clip_polygon(&polygon[..count], n, d, scratch);
                core::mem::swap(&mut polygon, &mut scratch);
                if count < 3 {
                    break;
                }
            }
            if count < 3 {
                continue;
            }
            #[cfg(feature = "tools")]
            {
                self.stats.visible += 1;
            }
            let normal = Vec3::new(
                tri.normal.dot(right),
                tri.normal.dot(up),
                tri.normal.dot(forward),
            );
            let project = |v: ClipVertex| {
                let inv_z = 1.0 / v.p.z;
                ScreenVertex {
                    x: (v.p.x * inv_z / tan_x + 1.0) * width as u32 as f32 * 0.5,
                    y: (1.0 - v.p.y * inv_z / tan_y) * height as u32 as f32 * 0.5,
                    inv_z,
                    uv_over_z: v.uv * inv_z,
                    light_over_z: v.light * inv_z,
                }
            };
            for i in 1..count - 1 {
                self.rasterize(
                    frame,
                    [
                        project(polygon[0]),
                        project(polygon[i]),
                        project(polygon[i + 1]),
                    ],
                    Shading {
                        lightmap: mesh.lightmaps.get(tri.lightmap as usize),
                        material: tri.material,
                        normal,
                        tan_x,
                        tan_y,
                        fog_color,
                    },
                );
            }
        }
    }
    fn rasterize(&mut self, frame: &mut Frame, v: [ScreenVertex; 3], shade: Shading<'_>) {
        let material = shade.material;
        let area = edge(v[0], v[1], v[2].x, v[2].y);
        if area.abs() < 1e-6 {
            return;
        }
        let min_x = v
            .iter()
            .map(|p| p.x)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32 as usize;
        let max_x = v
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .max(0.0)
            .min(frame.width as u32 as f32) as u32 as usize;
        let min_y = v
            .iter()
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32 as usize;
        let max_y = v
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .max(0.0)
            .min(frame.height as u32 as f32) as u32 as usize;
        let pixel_height = 2.0 * shade.tan_y / frame.height as u32 as f32;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let (px, py) = (x as u32 as f32 + 0.5, y as u32 as f32 + 0.5);
                let weights = [
                    edge(v[1], v[2], px, py) / area,
                    edge(v[2], v[0], px, py) / area,
                    edge(v[0], v[1], px, py) / area,
                ];
                if weights.iter().any(|w| *w < -0.00001) {
                    continue;
                }
                let inv_z =
                    weights[0] * v[0].inv_z + weights[1] * v[1].inv_z + weights[2] * v[2].inv_z;
                if inv_z <= 0.0 {
                    continue;
                }
                let z = 1.0 / inv_z;
                let index = y * frame.width + x;
                if z >= frame.depth[index] {
                    continue;
                }
                let uv = (v[0].uv_over_z * weights[0]
                    + v[1].uv_over_z * weights[1]
                    + v[2].uv_over_z * weights[2])
                    * z;
                let mut light = (weights[0] * v[0].light_over_z
                    + weights[1] * v[1].light_over_z
                    + weights[2] * v[2].light_over_z)
                    * z;
                if let Some(map) = shade.lightmap {
                    light = map.sample(uv);
                }
                let rx = (px / frame.width as u32 as f32 * 2.0 - 1.0) * shade.tan_x;
                let ry = (1.0 - py / frame.height as u32 as f32 * 2.0) * shade.tan_y;
                let distance_sq = z * z * (1.0 + rx * rx + ry * ry);
                let fade = ((196.0 - distance_sq) / 96.0).clamp(0.0, 1.0);
                let range = fade * fade * (3.0 - 2.0 * fade);
                if self.flashlight
                    && range > 0.0
                    && !matches!(
                        material,
                        Material::Light | Material::Green | Material::Exit | Material::Rage
                    )
                {
                    let direction = Vec3::new(rx, ry, 1.0).normalized();
                    let cone = ((direction.z - 0.79) / 0.17).clamp(0.0, 1.0);
                    light += 1.8 * cone * cone * range * (-shade.normal.dot(direction)).max(0.0)
                        / (1.0 + distance_sq * 0.12);
                }
                let mut base = texture(material, uv, z * pixel_height);
                if material == Material::Wire && self.flashlight {
                    let direction = Vec3::new(rx, ry, 1.0).normalized();
                    let cone = ((direction.z - 0.79) / 0.17).clamp(0.0, 1.0);
                    let spec = (-shade.normal.dot(direction)).max(0.0).powi(20)
                        * 0.016
                        * cone
                        * cone
                        * range
                        / (1.0 + distance_sq * 0.09);
                    for value in &mut base {
                        *value += spec;
                    }
                }
                let fog = if setting!(self, fog, true) {
                    fog_amount(distance_sq.sqrt())
                } else {
                    0.0
                };
                let vignette =
                    1.0 - 0.22 * ((rx / shade.tan_x).powi(2) + (ry / shade.tan_y).powi(2)).min(1.6);
                let color = core::array::from_fn::<_, 3, _>(|i| {
                    base[i] * light * setting!(self, exposure, 1.05) * vignette * (1.0 - fog)
                        + shade.fog_color[i] * fog
                });
                // At full fog the same colour is used for geometry and uncovered pixels.
                // Vignette and dithering fade away before the far clipping plane.
                frame.pixels[index] =
                    encode_color(color, x, y, setting!(self, retro, true), 1.0 - fog);
                frame.depth[index] = z;
                #[cfg(feature = "tools")]
                {
                    self.stats.fragments += 1;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Shading<'a> {
    lightmap: Option<&'a crate::mesh::Lightmap>,
    material: Material,
    normal: Vec3,
    tan_x: f32,
    tan_y: f32,
    fog_color: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Triangle, Vertex};
    fn camera() -> Camera {
        Camera {
            position: Vec3::default(),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov: 1.4,
        }
    }
    fn tri(z: f32, material: Material) -> Triangle {
        Triangle {
            vertices: [
                Vec3::new(-1.0, -1.0, z),
                Vec3::new(0.0, 1.0, z),
                Vec3::new(1.0, -1.0, z),
            ]
            .map(|p| Vertex {
                p,
                uv: Vec2::default(),
                light: 1.0,
            }),
            normal: Vec3::new(0.0, 0.0, -1.0),
            material,
            lightmap: u32::MAX,
        }
    }
    #[test]
    fn distant_geometry_fades_to_exactly_the_clear_colour_before_clipping() {
        for retro in [false, true] {
            let mut renderer = Renderer {
                retro,
                ..Default::default()
            };
            let empty = renderer.render(&Mesh::default(), camera(), 64, 40);
            for z in [96.0, 95.999, 96.01, 120.0] {
                let mut mesh = Mesh::default();
                mesh.add_quad(
                    [
                        Vec3::new(-100.0, -100.0, z),
                        Vec3::new(-100.0, 100.0, z),
                        Vec3::new(100.0, 100.0, z),
                        Vec3::new(100.0, -100.0, z),
                    ],
                    Material::Green,
                );
                let frame = renderer.render(&mesh, camera(), 64, 40);
                assert_eq!(
                    frame.pixels, empty.pixels,
                    "a far-plane hole appeared at {z}, retro={retro}"
                );
            }
        }
        assert_eq!(fog_amount(5.0), 0.0);
        assert!(fog_amount(20.0) < fog_amount(30.0));
        assert_eq!(fog_amount(96.0), 1.0);
    }
    #[test]
    fn an_unlit_room_needs_the_flashlight_and_fog_does_not_brighten_it() {
        use crate::{mesh::LIGHT_SIZE, mesh::Lightmap};
        let mut mesh = Mesh {
            light_width: 1,
            floor_light: vec![0],
            ..Mesh::default()
        };
        mesh.lightmaps.push(Lightmap {
            origin: Vec2::default(),
            scale: Vec2::new(1.0, 1.0),
            samples: [4; LIGHT_SIZE * LIGHT_SIZE],
        });
        mesh.add_quad(
            [
                Vec3::new(-2.0, -2.0, 3.0),
                Vec3::new(-2.0, 2.0, 3.0),
                Vec3::new(2.0, 2.0, 3.0),
                Vec3::new(2.0, -2.0, 3.0),
            ],
            Material::Wall,
        );
        for tri in &mut mesh.triangles {
            tri.lightmap = 0;
            for v in &mut tri.vertices {
                v.uv = Vec2::new(v.p.x, v.p.y + 2.0);
            }
        }
        let mut renderer = Renderer {
            retro: false,
            ..Default::default()
        };
        let dark = renderer.render(&mesh, camera(), 64, 40);
        let max = dark
            .pixels
            .iter()
            .map(|p| p.0.max(p.1).max(p.2))
            .max()
            .unwrap();
        assert!(max <= 5, "ambient/fog raised an unlit room to {max}");
        renderer.flashlight = true;
        let lit = renderer.render(&mesh, camera(), 64, 40);
        assert!(
            lit.pixels[20 * 64 + 32].0 > 80,
            "the torch must still illuminate the wall"
        );
    }
    #[test]
    fn wallpaper_has_a_continuous_integrated_black_base() {
        for x in [-10.0, 0.0, 2.0, 500.0] {
            for y in [0.0, 0.06, 0.119] {
                assert_eq!(
                    texture(Material::Wall, Vec2::new(x, y), 0.0),
                    [0.025, 0.029, 0.031]
                );
            }
        }
        assert_ne!(
            texture(Material::Wall, Vec2::new(0.0, 0.121), 0.0),
            [0.025, 0.029, 0.031]
        );
    }
    #[test]
    fn depth_occlusion_is_independent_of_draw_order() {
        let a = tri(2.0, Material::Exit);
        let b = tri(3.0, Material::Light);
        let mut r = Renderer::default();
        let f = r.render(
            &Mesh {
                triangles: vec![a, b],
                ..Mesh::default()
            },
            camera(),
            40,
            30,
        );
        let g = r.render(
            &Mesh {
                triangles: vec![b, a],
                ..Mesh::default()
            },
            camera(),
            40,
            30,
        );
        assert_eq!(f.pixels, g.pixels);
        assert!((f.depth[15 * 40 + 20] - 2.0).abs() < 1e-5);
    }
    #[test]
    fn triangles_crossing_near_plane_are_clipped_instead_of_dropped() {
        let mut t = tri(0.7, Material::Light);
        t.vertices[0].p.z = -0.3;
        t.normal = (t.vertices[1].p - t.vertices[0].p)
            .cross(t.vertices[2].p - t.vertices[0].p)
            .normalized();
        let mut r = Renderer::default();
        let frame = r.render(
            &Mesh {
                triangles: vec![t],
                ..Mesh::default()
            },
            camera(),
            60,
            40,
        );
        assert!(frame.depth.iter().any(|d| d.is_finite()));
        assert!(
            frame
                .depth
                .iter()
                .all(|d| d.is_infinite() || *d >= r.near - 0.001)
        );
    }
    #[test]
    fn geometry_behind_camera_never_appears() {
        let mut t = tri(-2.0, Material::Light);
        t.normal = Vec3::new(0.0, 0.0, 1.0);
        let frame = Renderer::default().render(
            &Mesh {
                triangles: vec![t],
                ..Mesh::default()
            },
            camera(),
            40,
            30,
        );
        assert!(frame.depth.iter().all(|d| d.is_infinite()));
    }
    #[test]
    fn a_uniform_surface_has_no_seam_at_former_lighting_zone_boundaries() {
        let mut mesh = Mesh::default();
        mesh.add_quad(
            [
                Vec3::new(8.0, -2.0, 15.0),
                Vec3::new(8.0, 2.0, 15.0),
                Vec3::new(12.0, 2.0, 15.0),
                Vec3::new(12.0, -2.0, 15.0),
            ],
            Material::Wall,
        );
        let mut camera = camera();
        camera.position = Vec3::new(10.0, 0.0, 11.0);
        let mut renderer = Renderer {
            retro: false,
            ..Default::default()
        };
        let frame = renderer.render(&mesh, camera, 64, 40);
        let (left, right) = (20 * 64 + 24, 20 * 64 + 39);
        assert!(frame.depth[left].is_finite() && frame.depth[right].is_finite());
        assert_eq!(frame.pixels[left], frame.pixels[right]);
    }

    #[test]
    fn stationary_world_lighting_is_stable_across_two_clock_cycles() {
        use crate::{player::Player, world::Map};
        let mut map = Map::generated(17, 42).unwrap();
        for z in 1..16 {
            for x in 1..16 {
                map.set_wall(x, z, false);
            }
        }
        map.spawn.x = 15.0;
        map.spawn.z = 15.0;
        let mesh = Mesh::from_map(&map);
        let camera = Player::new(&map).camera;
        for flashlight in [false, true] {
            let mut renderer = Renderer {
                flashlight,
                ..Default::default()
            };
            let reference = renderer.render(&mesh, camera, 64, 40);
            for second in 0..=124 {
                let frame = renderer.render(&mesh, camera, 64, 40);
                assert!(
                    reference.pixels == frame.pixels,
                    "static light changed, lamp={flashlight}, iteration={second}"
                );
                assert_eq!(reference.depth, frame.depth);
            }
        }
    }
    #[test]
    fn fixed_clipping_matches_reference_for_generated_triangles() {
        fn reference(input: &[ClipVertex], n: Vec3, d: f32) -> Vec<ClipVertex> {
            let mut out = Vec::new();
            if input.is_empty() {
                return out;
            }
            let mut prev = *input.last().unwrap();
            let mut pd = n.dot(prev.p) + d;
            for &cur in input {
                let cd = n.dot(cur.p) + d;
                if (pd >= 0.0) != (cd >= 0.0) {
                    out.push(prev.lerp(cur, pd / (pd - cd)));
                }
                if cd >= 0.0 {
                    out.push(cur);
                }
                prev = cur;
                pd = cd;
            }
            out
        }
        let planes = [
            (Vec3::new(0.0, 0.0, 1.0), -0.065),
            (Vec3::new(0.0, 0.0, -1.0), 64.0),
            (Vec3::new(1.0, 0.0, 0.9), 0.0),
            (Vec3::new(-1.0, 0.0, 0.9), 0.0),
            (Vec3::new(0.0, 1.0, 0.6), 0.0),
            (Vec3::new(0.0, -1.0, 0.6), 0.0),
        ];
        let mut state = 42u64;
        for _ in 0..4000 {
            let mut value = || {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((state >> 32) as f32 / u32::MAX as f32 - 0.5) * 180.0
            };
            let mut a = [ClipVertex::default(); CLIP_CAPACITY];
            let mut b = a;
            for vertex in &mut a[..3] {
                *vertex = ClipVertex {
                    p: Vec3::new(value(), value(), value()),
                    uv: Vec2::new(value(), value()),
                    light: value(),
                };
            }
            let mut expected = a[..3].to_vec();
            let mut count = 3;
            for (n, d) in planes {
                expected = reference(&expected, n, d);
                count = clip_polygon(&a[..count], n, d, &mut b);
                core::mem::swap(&mut a, &mut b);
                assert_eq!(&a[..count], expected.as_slice());
            }
        }
    }
    #[test]
    fn supersampling_keeps_terminal_size_and_camera_projection() {
        let mesh = Mesh {
            triangles: vec![tri(2.0, Material::Light)],
            ..Mesh::default()
        };
        let mut renderer = Renderer::default();
        let native = renderer.render(&mesh, camera(), 40, 30);
        let one = renderer.render_scaled(&mesh, camera(), 40, 30, 1);
        assert_eq!(one.pixels, native.pixels);
        assert_eq!(one.depth, native.depth);
        for scale in [2, 4, 8, 16, 32, MAX_RENDER_SCALE] {
            let aa = renderer.render_scaled(&mesh, camera(), 40, 30, scale);
            assert_eq!(
                (aa.width, aa.height, aa.pixels.len(), aa.depth.len()),
                (40, 30, 1200, 1200)
            );
            let center = 15 * 40 + 20;
            assert!((aa.depth[center] - native.depth[center]).abs() < 0.0001);
            assert_ne!(
                aa.pixels, native.pixels,
                "subpixel coverage should smooth edges"
            );
        }
    }
}
