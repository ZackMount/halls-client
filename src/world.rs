use crate::bitmap::Walls;
#[cfg(any(feature = "tools", test))]
use crate::error::bail;
use crate::error::{Context, Result};
use crate::math::Vec3;
#[allow(unused_imports)]
use crate::prelude::*;
use alloc::collections::VecDeque;
#[cfg(feature = "tools")]
use std::{fs, path::Path};
unsafe extern "C" {
    fn halls_random(value: *mut u64) -> i32;
}
pub fn fresh_seed() -> u64 {
    let mut value = 0;
    assert_eq!(
        unsafe { halls_random(&mut value) },
        0,
        "System randomness unavailable"
    );
    value
}
fn random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 24
}

#[derive(Clone, Debug)]
pub struct Spawn {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
}
#[cfg(any(feature = "tools", test))]
#[derive(Clone, Debug)]
pub struct Elevation {
    pub cell: [usize; 2],
    pub height: f32,
    /// Height difference across one tile in the east and south directions.
    pub slope: [f32; 2],
}
#[cfg(any(feature = "tools", test))]
#[derive(Clone, Debug)]
pub struct Block {
    pub min: [f32; 3],
    pub max: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct Hall {
    pub min: [usize; 2],
    pub size: [usize; 2],
    pub doors: Vec<[usize; 2]>,
    /// Render-only ceiling layout; never participates in map or gameplay RNG.
    pub light_profile: u8,
    pub light_seed: u64,
}
#[derive(Clone, Debug)]
pub struct Map {
    #[cfg(any(feature = "tools", test))]
    pub version: u32,
    pub walls: Walls,
    pub spawn: Spawn,
    pub exit: [usize; 2],
    pub tile_size: f32,
    pub ceiling: f32,
    pub eye_height: f32,
    #[cfg(any(feature = "tools", test))]
    pub elevations: Vec<Elevation>,
    #[cfg(any(feature = "tools", test))]
    pub blocks: Vec<Block>,
    pub seed: u64,
    #[cfg(any(feature = "tools", test))]
    pub breakers: Vec<[usize; 2]>,
    pub entities: Vec<[usize; 2]>,
    pub halls: Vec<Hall>,
}

impl Map {
    /// World-space bounds of the full green exit tile: [min_x, min_z, max_x, max_z].
    pub fn exit_bounds(&self) -> [f32; 4] {
        let x = self.exit[0] as f32 * self.tile_size;
        let z = self.exit[1] as f32 * self.tile_size;
        [x, z, x + self.tile_size, z + self.tile_size]
    }
    pub fn in_exit(&self, x: f32, z: f32) -> bool {
        let [x0, z0, x1, z1] = self.exit_bounds();
        x >= x0 && x < x1 && z >= z0 && z < z1
    }
    pub fn width(&self) -> usize {
        self.walls.width
    }
    pub fn height(&self) -> usize {
        self.walls.height
    }
    pub fn wall(&self, x: i32, z: i32) -> bool {
        x < 0
            || z < 0
            || z as usize >= self.height()
            || x as usize >= self.width()
            || self.walls.get(x as usize, z as usize)
    }
    #[cfg(any(feature = "tools", test))]
    pub fn elevation(&self, x: usize, z: usize) -> Option<&Elevation> {
        self.elevations.iter().find(|e| e.cell == [x, z])
    }
    #[cfg(any(feature = "tools", test))]
    pub fn floor_at(&self, x: f32, z: f32) -> f32 {
        let gx = (x / self.tile_size).floor();
        let gz = (z / self.tile_size).floor();
        if gx < 0.0 || gz < 0.0 {
            return 0.0;
        }
        self.elevation(gx as usize, gz as usize).map_or(0.0, |e| {
            e.height
                + e.slope[0] * (x / self.tile_size - gx)
                + e.slope[1] * (z / self.tile_size - gz)
        })
    }
    #[cfg(any(feature = "tools", test))]
    pub fn cell_floor(&self, x: usize, z: usize, dx: f32, dz: f32) -> f32 {
        self.elevation(x, z)
            .map_or(0.0, |e| e.height + e.slope[0] * dx + e.slope[1] * dz)
    }
    #[cfg(not(any(feature = "tools", test)))]
    pub fn floor_at(&self, _x: f32, _z: f32) -> f32 {
        0.0
    }
    #[cfg(not(any(feature = "tools", test)))]
    pub fn cell_floor(&self, _x: usize, _z: usize, _dx: f32, _dz: f32) -> f32 {
        0.0
    }
    pub fn spawn_position(&self) -> Vec3 {
        Vec3::new(
            self.spawn.x,
            self.floor_at(self.spawn.x, self.spawn.z) + self.eye_height,
            self.spawn.z,
        )
    }
    pub fn set_wall(&mut self, x: usize, z: usize, solid: bool) {
        self.walls.set(x, z, solid);
        #[cfg(any(feature = "tools", test))]
        if solid {
            self.elevations.retain(|e| e.cell != [x, z]);
        }
    }
    #[cfg(feature = "tools")]
    pub fn load(path: &Path) -> Result<Self> {
        let map: Self = crate::map_file::load(
            &fs::read(path).with_context(|| format!("Reading {}", path.display()))?,
        )?;
        #[cfg(any(feature = "tools", test))]
        map.validate()?;
        Ok(map)
    }
    #[cfg(feature = "tools")]
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        // Commit only a complete file; an interrupted write must not truncate the editor's map.
        let temp = path.with_extension("json.tmp");
        let mut bytes = crate::map_file::save(self);
        bytes.push(b'\n');
        fs::write(&temp, bytes)?;
        if path.exists() {
            fs::copy(path, path.with_extension("json.bak"))?;
        }
        fs::rename(&temp, path)?;
        Ok(())
    }
    #[cfg(any(feature = "tools", test))]
    pub fn validate(&self) -> Result<()> {
        if self.version != 2 {
            bail!("Unsupported map version {}", self.version);
        }
        let (w, h) = (self.width(), self.height());
        if !(3..=256).contains(&w) || !(3..=256).contains(&h) {
            bail!("Map must be between 3x3 and 256x256");
        }
        if !self.tile_size.is_finite()
            || !(1.0..=8.0).contains(&self.tile_size)
            || !self.ceiling.is_finite()
            || !(2.0..=12.0).contains(&self.ceiling)
        {
            bail!("Invalid world dimensions");
        }
        if !self.eye_height.is_finite() || !(0.8..=2.0).contains(&self.eye_height) {
            bail!("Invalid eye height");
        }
        if !self.spawn.x.is_finite() || !self.spawn.z.is_finite() || !self.spawn.yaw.is_finite() {
            bail!("Invalid spawn");
        }
        let sx = (self.spawn.x / self.tile_size).floor() as i32;
        let sz = (self.spawn.z / self.tile_size).floor() as i32;
        if self.wall(sx, sz) {
            bail!("Spawn must be inside an open cell");
        }
        if self.exit[0] >= w
            || self.exit[1] >= h
            || self.wall(self.exit[0] as i32, self.exit[1] as i32)
        {
            bail!("Exit must be inside an open cell");
        }
        let mut seen = vec![false; w * h];
        for e in &self.elevations {
            if e.cell[0] >= w
                || e.cell[1] >= h
                || self.wall(e.cell[0] as i32, e.cell[1] as i32)
                || seen[e.cell[1] * w + e.cell[0]]
            {
                bail!("Invalid or duplicate elevation cell");
            }
            seen[e.cell[1] * w + e.cell[0]] = true;
            if !e.height.is_finite()
                || e.slope
                    .iter()
                    .any(|s| !s.is_finite() || s.abs() > self.tile_size * 0.6)
            {
                bail!("Invalid floor slope");
            }
            for (dx, dz) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let y = e.height + e.slope[0] * dx + e.slope[1] * dz;
                if !(0.0..=self.ceiling - self.eye_height - 0.2).contains(&y) {
                    bail!("Floor is outside the playable height range");
                }
            }
        }
        for &cell in &self.entities {
            if cell[0] >= w || cell[1] >= h || self.wall(cell[0] as i32, cell[1] as i32) {
                bail!("Invalid entity spawn");
            }
            let x = (cell[0] as f32 + 0.5) * self.tile_size;
            let z = (cell[1] as f32 + 0.5) * self.tile_size;
            if !self.walkable(x, z, self.floor_at(x, z)) {
                bail!("Entity spawn intersects geometry");
            }
        }
        for b in &self.blocks {
            if b.min.iter().chain(&b.max).any(|v| !v.is_finite())
                || (0..3).any(|i| b.min[i] >= b.max[i])
                || b.min[0] < 0.0
                || b.min[2] < 0.0
                || b.max[0] > w as f32 * self.tile_size
                || b.max[2] > h as f32 * self.tile_size
                || b.min[1] < 0.0
                || b.max[1] > self.ceiling
            {
                bail!("Invalid solid block");
            }
        }
        if !self.walkable(
            self.spawn.x,
            self.spawn.z,
            self.floor_at(self.spawn.x, self.spawn.z),
        ) {
            bail!("Spawn is too close to a wall or intersects geometry");
        }
        if self.shortest_route().is_none() {
            bail!("Exit is unreachable by the player");
        }
        let reachable = self.distances();
        let mut used = vec![];
        for &cell in &self.breakers {
            if cell[0] >= w
                || cell[1] >= h
                || self.wall(cell[0] as i32, cell[1] as i32)
                || reachable[cell[1] * w + cell[0]] == u16::MAX
                || used.contains(&cell)
            {
                bail!("Invalid or unreachable breaker");
            }
            used.push(cell);
            let mut target = self.clone();
            target.exit = cell;
            if target.shortest_route().is_none() {
                bail!("Breaker cannot be reached by the player");
            }
        }
        Ok(())
    }
    pub fn walkable(&self, x: f32, z: f32, current_floor: f32) -> bool {
        self.walkable_radius(x, z, current_floor, 0.22)
    }
    pub(crate) fn walkable_radius(&self, x: f32, z: f32, current_floor: f32, r: f32) -> bool {
        for (dx, dz) in [(0.0, 0.0), (-r, -r), (r, -r), (-r, r), (r, r)] {
            if self.wall(
                ((x + dx) / self.tile_size).floor() as i32,
                ((z + dz) / self.tile_size).floor() as i32,
            ) {
                return false;
            }
            #[cfg(any(feature = "tools", test))]
            {
                let y = self.floor_at(x + dx, z + dz);
                if y - current_floor > 0.24 || self.ceiling - y < self.eye_height + 0.12 {
                    return false;
                }
            }
        }
        #[cfg(any(feature = "tools", test))]
        {
            let feet = self.floor_at(x, z);
            !self.blocks.iter().any(|b| {
                x + r > b.min[0]
                    && x - r < b.max[0]
                    && z + r > b.min[2]
                    && z - r < b.max[2]
                    && feet + self.eye_height > b.min[1]
                    && feet < b.max[1]
            })
        }
        #[cfg(not(any(feature = "tools", test)))]
        {
            let _ = current_floor;
            true
        }
    }
    /// Deterministic BFS, with movement feasibility sampled across each tile boundary.
    #[cfg(any(feature = "tools", test))]
    pub fn shortest_route(&self) -> Option<Vec<u8>> {
        let w = self.width();
        let h = self.height();
        if w == 0 || h == 0 {
            return None;
        }
        let start = ((self.spawn.z / self.tile_size).floor() as usize) * w
            + (self.spawn.x / self.tile_size).floor() as usize;
        let end = self.exit[1] * w + self.exit[0];
        let mut parent = vec![None; w * h];
        let mut q = VecDeque::from([start]);
        parent[start] = Some((start, 0));
        while let Some(at) = q.pop_front() {
            if at == end {
                let mut route = vec![];
                let mut cur = end;
                while cur != start {
                    let (p, d) = parent[cur]?;
                    route.push(d);
                    cur = p;
                }
                route.reverse();
                return Some(route);
            }
            let (x, z) = (at % w, at / w);
            for (dx, dz, d) in [(0, -1, b'N'), (1, 0, b'E'), (0, 1, b'S'), (-1, 0, b'W')] {
                let nx = x as i32 + dx;
                let nz = z as i32 + dz;
                if self.wall(nx, nz) {
                    continue;
                }
                let index = nz as usize * w + nx as usize;
                if parent[index].is_some() {
                    continue;
                }
                let mut px = (x as u32 as f32 + 0.5) * self.tile_size;
                let mut pz = (z as u32 as f32 + 0.5) * self.tile_size;
                let steps = (self.tile_size / 0.08).ceil() as usize;
                let mut valid = true;
                for _ in 0..steps {
                    let floor = self.floor_at(px, pz);
                    px += dx as f32 * self.tile_size / steps as u32 as f32;
                    pz += dz as f32 * self.tile_size / steps as u32 as f32;
                    if !self.walkable(px, pz, floor) {
                        valid = false;
                        break;
                    }
                }
                if valid {
                    parent[index] = Some((at, d));
                    q.push_back(index);
                }
            }
        }
        None
    }
    pub fn light_visible(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let steps = (d.length() * 3.0).ceil().max(1.0) as usize;
        (1..steps).all(|i| {
            let p = a + d * (i as u32 as f32 / steps as u32 as f32);
            if self.wall(
                (p.x / self.tile_size).floor() as i32,
                (p.z / self.tile_size).floor() as i32,
            ) {
                return false;
            }
            #[cfg(any(feature = "tools", test))]
            {
                if self.blocks.iter().any(|b| {
                    p.x > b.min[0]
                        && p.x < b.max[0]
                        && p.y > b.min[1]
                        && p.y < b.max[1]
                        && p.z > b.min[2]
                        && p.z < b.max[2]
                }) {
                    return false;
                }
            }
            true
        })
    }
    #[cfg(any(feature = "tools", test))]
    fn maze(size: usize, seed: u64) -> Self {
        let mut state = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut random = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut cells = Walls::new(size, size);
        cells.set(1, 1, false);
        let mut stack = vec![(1usize, 1usize)];
        while let Some(&(x, z)) = stack.last() {
            let mut options = [(0, 0); 4];
            let mut count = 0;
            for (dx, dz) in [(0, -2), (2, 0), (0, 2), (-2, 0)] {
                let nx = x as i32 + dx;
                let nz = z as i32 + dz;
                if nx > 0
                    && nz > 0
                    && nx < (size - 1) as i32
                    && nz < (size - 1) as i32
                    && cells.get(nx as usize, nz as usize)
                {
                    options[count] = (nx as usize, nz as usize);
                    count += 1;
                }
            }
            if count == 0 {
                stack.pop();
            } else {
                let (nx, nz) = options[random() as usize % count];
                cells.set((x + nx) / 2, (z + nz) / 2, false);
                cells.set(nx, nz, false);
                stack.push((nx, nz));
            }
        }
        // A small arrival room, with a clean view of the actual geometry.
        for z in 1..4 {
            for x in 1..4 {
                cells.set(x, z, false);
            }
        }
        Self {
            #[cfg(any(feature = "tools", test))]
            version: 2,
            walls: cells,
            spawn: Spawn {
                x: 3.0,
                z: 3.0,
                yaw: core::f32::consts::FRAC_PI_2,
            },
            exit: [size - 2, size - 2],
            tile_size: 2.0,
            ceiling: 3.4,
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
        }
    }
    #[cfg(any(feature = "tools", test))]
    pub fn generated(size: usize, seed: u64) -> Result<Self> {
        if !(7..=256).contains(&size) {
            bail!("Invalid fixture dimensions");
        }
        let mut map = Self::maze(size, seed);
        // The farthest reachable open tile becomes the exit.
        let mut distances = vec![u16::MAX; size * size];
        let mut q = VecDeque::from([(1usize, 1usize)]);
        distances[size + 1] = 0;
        let mut far = (1, 1);
        while let Some((x, z)) = q.pop_front() {
            if distances[z * size + x] > distances[far.1 * size + far.0] {
                far = (x, z);
            }
            for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let nx = x as i32 + dx;
                let nz = z as i32 + dz;
                if !map.wall(nx, nz) {
                    let (nx, nz) = (nx as usize, nz as usize);
                    if distances[nz * size + nx] == u16::MAX {
                        distances[nz * size + nx] = distances[z * size + x] + 1;
                        q.push_back((nx, nz));
                    }
                }
            }
        }
        map.exit = [far.0, far.1];
        #[cfg(any(feature = "tools", test))]
        map.validate()?;
        Ok(map)
    }

    pub fn distances(&self) -> Vec<u16> {
        let w = self.width();
        let mut distances = vec![u16::MAX; w * self.height()];
        let start = [
            (self.spawn.x / self.tile_size) as usize,
            (self.spawn.z / self.tile_size) as usize,
        ];
        let mut q = VecDeque::from([(start[1] * w + start[0]) as u16]);
        distances[start[1] * w + start[0]] = 0;
        while let Some(at) = q.pop_front() {
            let (x, z) = (at as usize % w, at as usize / w);
            for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                if self.wall(nx, nz) {
                    continue;
                }
                let (nx, nz) = (nx as usize, nz as usize);
                let index = nz * w + nx;
                if distances[index] == u16::MAX {
                    distances[index] = distances[z * w + x] + 1;
                    q.push_back(index as u16);
                }
            }
        }
        distances
    }

    fn populate_from_distances(&mut self, distances: &[u16]) {
        let w = self.width();
        let h = self.height();
        let safe = |i: usize| {
            let x = (i % w) as f32 * self.tile_size + self.tile_size * 0.5;
            let z = (i / w) as f32 * self.tile_size + self.tile_size * 0.5;
            distances[i] != u16::MAX
                && distances[i] as f32 * self.tile_size >= 96.0
                && (x - self.spawn.x).hypot(z - self.spawn.z) >= 64.0
                && [i % w, i / w] != self.exit
        };
        let candidates: Vec<_> = (0..distances.len()).filter(|&i| safe(i)).collect();
        // Descending the existing distance field recovers one actual shortest
        // route, without another search or any change to the room geometry.
        let mut at = self.exit[1] * w + self.exit[0];
        let total = distances[at];
        let mut middle = None;
        let mut best = usize::MAX;
        while distances[at] > 0 && distances[at] != u16::MAX {
            let d = distances[at];
            if d >= total / 3 && d <= total - total / 3 && safe(at) {
                let score = (at % w).abs_diff(w / 2).pow(2) + (at / w).abs_diff(h / 2).pow(2);
                if score < best {
                    best = score;
                    middle = Some(at);
                }
            }
            let x = at % w;
            let z = at / w;
            let next = [
                (x, z.wrapping_sub(1)),
                (x + 1, z),
                (x, z + 1),
                (x.wrapping_sub(1), z),
            ]
            .into_iter()
            .find(|&(x, z)| x < w && z < h && distances[z * w + x] == d - 1);
            let Some((x, z)) = next else { break };
            at = z * w + x;
        }
        self.entities.clear();
        let Some(middle) = middle.or_else(|| candidates.first().copied()) else {
            return;
        };
        // Four outlying patrols stay in separate quadrants. Avoid clustering
        // around the route patrol, while keeping every spawn well away from the player.
        for quadrant in 0..4 {
            let (qx, qz) = (quadrant & 1, quadrant >> 1);
            let center = [(1 + 2 * qx) * w / 4, (1 + 2 * qz) * h / 4];
            let chosen = candidates
                .iter()
                .copied()
                .filter(|&i| {
                    i != middle
                        && usize::from(i % w >= w / 2) == qx
                        && usize::from(i / w >= h / 2) == qz
                })
                .min_by_key(|&i| {
                    let near = (w / 8).saturating_sub(
                        (i % w).abs_diff(middle % w) + (i / w).abs_diff(middle / w),
                    );
                    (i % w).abs_diff(center[0]).pow(2)
                        + (i / w).abs_diff(center[1]).pow(2)
                        + near * w
                });
            if let Some(i) = chosen {
                self.entities.push([i % w, i / w]);
            }
        }
        self.entities.push([middle % w, middle / w]);
    }
    pub fn randomize_player_spawn(&mut self, seed: u64) -> Result<()> {
        let connected = self.distances();
        let w = self.width();
        let mut state = seed ^ 0xd1b54a32d192ed03;
        let mut candidates = vec![];
        for hall in &self.halls {
            let mx = (hall.size[0] / 4).max(1);
            let mz = (hall.size[1] / 4).max(1);
            for z in hall.min[1] + mz..hall.min[1] + hall.size[1] - mz {
                for x in hall.min[0] + mx..hall.min[0] + hall.size[0] - mx {
                    if !self.wall(x as i32, z as i32) && connected[z * w + x] != u16::MAX {
                        candidates.push([x, z]);
                    }
                }
            }
        }
        if candidates.is_empty() {
            for z in 1..self.height() - 1 {
                for x in 1..w - 1 {
                    if !self.wall(x as i32, z as i32) && connected[z * w + x] != u16::MAX {
                        candidates.push([x, z]);
                    }
                }
            }
        }
        // Shuffle, then reject unsafe cells rather than selecting a bad start.
        for i in (1..candidates.len()).rev() {
            let j = random(&mut state) as usize % (i + 1);
            candidates.swap(i, j);
        }
        let cell = candidates
            .into_iter()
            .find(|&[x, z]| {
                let px = (x as u32 as f32 + 0.5) * self.tile_size;
                let pz = (z as u32 as f32 + 0.5) * self.tile_size;
                let openings = [(0, -1), (1, 0), (0, 1), (-1, 0)]
                    .into_iter()
                    .filter(|&(dx, dz)| !self.wall(x as i32 + dx, z as i32 + dz))
                    .count();
                openings >= 2 && self.walkable(px, pz, self.floor_at(px, pz))
            })
            .context("No safe player spawn")?;
        self.spawn.x = (cell[0] as f32 + 0.5) * self.tile_size;
        self.spawn.z = (cell[1] as f32 + 0.5) * self.tile_size;
        let mut directions = [0u8; 4];
        let mut count = 0;
        for (direction, (dx, dz)) in [(0, 1), (1, 0), (0, -1), (-1, 0)].into_iter().enumerate() {
            if !self.wall(cell[0] as i32 + dx, cell[1] as i32 + dz) {
                directions[count] = direction as u8;
                count += 1;
            }
        }
        self.spawn.yaw =
            directions[random(&mut state) as usize % count] as f32 * core::f32::consts::FRAC_PI_2;
        let distances = self.distances();
        let end = distances
            .iter()
            .enumerate()
            .filter(|(_, d)| **d != u16::MAX)
            .max_by_key(|(_, d)| **d)
            .map(|(i, _)| i)
            .context("No reachable exit")?;
        self.exit = [end % w, end / w];
        self.populate_from_distances(&distances);
        #[cfg(any(feature = "tools", test))]
        {
            self.validate()
        }
        #[cfg(not(any(feature = "tools", test)))]
        {
            Ok(())
        }
    }
    /// The shipped game always generates a 256 x 256 map.
    pub fn procedural(seed: u64) -> Result<Self> {
        crate::map_generator::generate(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_maps_have_repeatable_reachable_exits() {
        for seed in [0, 1, 73, u64::MAX] {
            let m = Map::generated(17, seed).unwrap();
            assert!(m.shortest_route().unwrap().len() > 12);
            assert_eq!(m.walls, Map::generated(17, seed).unwrap().walls);
        }
    }
    #[test]
    fn rejects_unreachable_exit() {
        let mut m = Map::generated(7, 1).unwrap();
        m.exit = [3, 3];
        m.set_wall(3, 3, false);
        for (x, z) in [(2, 3), (4, 3), (3, 2), (3, 4)] {
            m.set_wall(x, z, true);
        }
        assert!(m.validate().is_err());
    }
    #[test]
    fn elevation_blocks_large_steps_but_allows_ramps() {
        let mut m = Map::generated(7, 1).unwrap();
        m.elevations.push(Elevation {
            cell: [2, 1],
            height: 0.8,
            slope: [0.0, 0.0],
        });
        assert!(!m.walkable(4.1, 3.0, 0.0));
        m.elevations[0].height = 0.0;
        m.elevations[0].slope = [0.8, 0.0];
        assert!(m.walkable(4.1, 3.0, 0.0));
    }
    #[test]
    fn new_runs_obtain_independent_system_randomness() {
        let values: std::collections::HashSet<_> = (0..64).map(|_| fresh_seed()).collect();
        assert_eq!(values.len(), 64);
        let a = Map::procedural(fresh_seed()).unwrap();
        let b = Map::procedural(fresh_seed()).unwrap();
        assert_ne!(a.walls, b.walls);
    }
}
