#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    math::Vec3,
    mesh::{Material, Mesh},
    player::Player,
    world::Map,
};
// Stored goal-first: popping the next waypoint requires no ring-buffer machinery.
#[derive(Clone, Debug, Default)]
pub struct Route(Vec<u16>);
impl Route {
    fn new() -> Self {
        Self(Vec::new())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &u16> {
        self.0.iter().rev()
    }
    pub fn front(&self) -> Option<&u16> {
        self.0.last()
    }
    pub fn back(&self) -> Option<&u16> {
        self.0.first()
    }
    fn clear(&mut self) {
        self.0.clear();
    }
    fn push_front(&mut self, node: u16) {
        self.0.push(node);
    }
    fn pop_front(&mut self) -> Option<u16> {
        self.0.pop()
    }
}

pub const ATTACK_DURATION: f32 = 0.40;
pub const WINDUP_TIME: f32 = 0.06;
pub const STRIKE_TIME: f32 = 0.10;
pub const ATTACK_COOLDOWN: f32 = 0.99;
pub const HEARING_WALK: f32 = 3.0;
pub const HEARING_RUN: f32 = 32.0;
pub const TRACK_RANGE: f32 = 28.0;
pub const HATE_GAIN: f32 = 65.0;
pub const HATE_DECAY: f32 = 6.0;
pub const CONTACT_GRACE: f32 = 4.0;
pub const CHASE_SPEED: f32 = 6.5;
const CHASE_ENTER: f32 = 35.0;
const CHASE_LEAVE: f32 = 20.0;
const LIVE_TRACK_MIN: f32 = 70.0;
const BODY_RADIUS: f32 = 0.26;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Roam,
    Investigate,
    Chase,
    Search,
}
#[derive(Clone, Debug)]
pub struct Entity {
    pub position: [f32; 2],
    pub yaw: f32,
    pub mood: Mood,
    pub target: [usize; 2],
    pub last_seen: f32,
    pub cooldown: f32,
    pub replan: f32,
    pub idle: f32,
    pub seed: u64,
    pub path: Route,
    pub travel: f32,
    pub move_blend: f32,
    pub attack_left: f32,
    pub strike_done: bool,
    pub sight_charge: f32,
    pub hate: f32,
    pub speed: f32,
    pub last_known: [f32; 2],
    pub form_seed: u64,
    pub patrol_anchor: Option<[usize; 2]>,
    patrol_age: f32,
    patrol_drift: f32,
    patrol_span: u8,
}
pub fn spawn(map: &Map) -> Vec<Entity> {
    spawn_with_seed(map, crate::world::fresh_seed())
}
pub fn spawn_with_seed(map: &Map, run_seed: u64) -> Vec<Entity> {
    #[cfg(any(feature = "tools", test))]
    let distances = map.distances();
    #[cfg(any(feature = "tools", test))]
    let mut candidates: Vec<_> = map
        .entities
        .iter()
        .copied()
        .filter(|cell| {
            if cell[0] >= map.width() || cell[1] >= map.height() {
                return false;
            }
            let x = (cell[0] as u32 as f32 + 0.5) * map.tile_size;
            let z = (cell[1] as u32 as f32 + 0.5) * map.tile_size;
            (x - map.spawn.x).hypot(z - map.spawn.z) >= 64.0
                && distances[cell[1] * map.width() + cell[0]] != u16::MAX
                && distances[cell[1] * map.width() + cell[0]] as f32 * map.tile_size >= 96.0
        })
        .collect();
    // Procedural map construction already enforces both spawn distances.
    #[cfg(not(any(feature = "tools", test)))]
    let mut candidates = map.entities.clone();
    let mut random = run_seed ^ map.seed ^ 0xa0761d6478bd642f;
    for i in (1..candidates.len()).rev() {
        random = random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        candidates.swap(i, (random >> 32) as usize % (i + 1));
    }
    let count = if map.entities.len() == 5 {
        5
    } else {
        map.width().div_ceil(64).clamp(1, 4)
    }
    .min(candidates.len());
    let separation = (map.width() / 3).max(8);
    // Keep the first shuffled spawn; spread the rest away from selected entities.
    for picked in 1..count {
        let mut next = picked;
        let mut best = 0;
        for at in picked..candidates.len() {
            let cell = candidates[at];
            let score = candidates[..picked].iter().fold(usize::MAX, |min, from| {
                min.min(from[0].abs_diff(cell[0]) + from[1].abs_diff(cell[1]))
            });
            if score >= separation {
                next = at;
                break;
            }
            if score > best {
                next = at;
                best = score;
            }
        }
        candidates.swap(picked, next);
    }
    candidates.truncate(count);
    candidates
        .iter()
        .enumerate()
        .map(|(i, &cell)| {
            let mut entity = Entity::at_cell(
                map,
                cell,
                run_seed ^ map.seed ^ (i as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15),
            );
            entity.yaw = (entity.random() % 65536) as u32 as f32 * core::f32::consts::TAU / 65536.0;
            entity.idle = 0.5 + (entity.random() % 2000) as u32 as f32 * 0.001;
            if map.entities.len() == 5 && map.entities.last() == Some(&cell) {
                entity.patrol_anchor = Some(cell);
            }
            entity
        })
        .collect()
}
impl Entity {
    pub fn at_cell(map: &Map, cell: [usize; 2], seed: u64) -> Self {
        let position = [
            (cell[0] as u32 as f32 + 0.5) * map.tile_size,
            (cell[1] as u32 as f32 + 0.5) * map.tile_size,
        ];
        Self {
            position,
            yaw: 0.0,
            mood: Mood::Roam,
            target: cell,
            last_seen: 0.0,
            cooldown: 0.0,
            replan: 0.0,
            idle: 0.0,
            seed,
            path: Route::new(),
            travel: 0.0,
            move_blend: 0.0,
            attack_left: 0.0,
            strike_done: false,
            sight_charge: 0.0,
            hate: 0.0,
            speed: 0.0,
            last_known: position,
            form_seed: seed.max(1),
            patrol_anchor: None,
            patrol_age: 0.0,
            patrol_drift: 0.0,
            patrol_span: 0,
        }
    }
    #[inline(never)]
    fn random(&mut self) -> u64 {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.seed >> 24
    }
    pub fn rage(&self) -> f32 {
        ((self.hate - LIVE_TRACK_MIN) / (100.0 - LIVE_TRACK_MIN)).bound(0.0, 1.0)
    }
    fn remember(&mut self, position: Vec3, map: &Map) {
        self.last_known = [position.x, position.z];
        self.target = cell_at(self.last_known, map);
    }
    fn record_motion(&mut self, before: [f32; 2], dt: f32) {
        let moved = norm2(self.position[0] - before[0], self.position[1] - before[1]);
        self.travel += moved;
        self.move_blend +=
            ((moved / dt.hi(0.001) / 3.0).bound(0.0, 1.0) - self.move_blend) * (dt * 10.0).lo(1.0);
    }
    #[inline(never)]
    fn advance(&mut self, map: &Map, direction: Vec3, distance: f32) -> f32 {
        let count = (distance / 0.07) as u32 as usize + 1;
        let step = direction * (distance / count as u32 as f32);
        let before = self.position;
        for _ in 0..count {
            let next = [self.position[0] + step.x, self.position[1] + step.z];
            if !body_fits(map, next, map.floor_at(self.position[0], self.position[1])) {
                // A glancing collision can slide along the free axis. Replan
                // on the next tick instead of repeatedly charging this corner.
                let floor = map.floor_at(self.position[0], self.position[1]);
                let sideways = [self.position[0] + step.x, self.position[1]];
                if body_fits(map, sideways, floor) {
                    self.position = sideways;
                }
                let forward = [self.position[0], self.position[1] + step.z];
                if body_fits(map, forward, floor) {
                    self.position = forward;
                }
                self.speed *= 0.625;
                self.path.clear();
                self.replan = 0.0;
                break;
            }
            self.position = next;
        }
        norm2(self.position[0] - before[0], self.position[1] - before[1])
    }
    fn walk_towards(
        &mut self,
        map: &Map,
        goal: [f32; 2],
        dt: f32,
        speed: f32,
        turn_rate: f32,
        stop: f32,
    ) -> f32 {
        let dx = goal[0] - self.position[0];
        let dz = goal[1] - self.position[1];
        let length = norm2(dx, dz);
        if length <= stop + 0.001 {
            self.speed = (self.speed - dt * 20.0).hi(0.0);
            return dt;
        }
        turn_towards(&mut self.yaw, dx.atan2(dz), dt, turn_rate);
        let direction = Vec3::new(dx / length, 0., dz / length);
        let alignment = (self.yaw.sin() * direction.x + self.yaw.cos() * direction.z).hi(0.);
        let squared = alignment * alignment;
        // Retain 75% of the former angular loss, without increasing turn rate.
        let wanted = speed * (0.37 + 0.63 * (squared * squared));
        let rate = if wanted < self.speed { 20.0 } else { 4.5 };
        self.speed += (wanted - self.speed).bound(-rate * dt, rate * dt);
        let distance = (self.speed * dt).lo((length - stop).hi(0.0));
        let moved = self.advance(map, direction, distance);
        (dt - moved / self.speed.hi(0.01)).hi(0.0)
    }
    #[inline(never)]
    fn reaches(&self, map: &Map, player: &Player, range: f32, angle: f32) -> bool {
        let at = Vec3::new(
            self.position[0],
            map.floor_at(self.position[0], self.position[1]) + 2.05,
            self.position[1],
        );
        let dx = player.camera.position.x - at.x;
        let dz = player.camera.position.z - at.z;
        let distance = norm2(dx, dz);
        distance < range
            && self.yaw.sin() * dx + self.yaw.cos() * dz > angle * distance
            && map.light_visible(at, player.camera.position)
    }
    pub fn tick(
        &mut self,
        map: &Map,
        player: &Player,
        seconds: f32,
        dt: f32,
        noise: f32,
        flashlight: bool,
    ) -> bool {
        let p = Vec3::new(
            self.position[0],
            map.floor_at(self.position[0], self.position[1]) + 2.05,
            self.position[1],
        );
        let delta = player.camera.position - p;
        let horizontal = Vec3::new(delta.x, 0.0, delta.z);
        let distance = horizontal.length();
        let forward = Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
        let visible = seconds > 6.0
            && distance < if flashlight { 12.0 } else { 7.0 }
            && (distance < 1.0 || forward.dot(horizontal.normalized()) > 0.80)
            && map.light_visible(p, player.camera.position);
        let previous = self.mood;
        let previous_target = self.target;
        let before = self.position;
        self.cooldown = (self.cooldown - dt).hi(0.0);
        self.replan -= dt;
        self.idle -= dt;
        self.sight_charge = if visible {
            (self.sight_charge + dt).lo(0.40)
        } else {
            (self.sight_charge - dt * 2.0).hi(0.0)
        };
        let spotted = visible && (self.sight_charge >= 0.18 || distance < 1.0);
        let heard = seconds > 6.0
            && noise > 0.0
            && distance <= noise
            && audible(map, self.position, player.camera.position, noise);
        // Only a previously acquired target receives this wider retention range.
        let tracked = matches!(self.mood, Mood::Chase | Mood::Search)
            && self.hate > CHASE_LEAVE
            && distance <= TRACK_RANGE
            && audible(map, self.position, player.camera.position, TRACK_RANGE);
        let contact = spotted || heard || tracked;
        if contact {
            self.last_seen = seconds;
            if spotted || tracked {
                self.hate = (self.hate.hi(if spotted { CHASE_ENTER } else { 0.0 })
                    + HATE_GAIN * dt)
                    .lo(100.0);
            } else {
                self.hate = (self.hate + 8.0).lo(100.0);
            }
            if spotted || self.hate >= CHASE_ENTER {
                self.mood = Mood::Chase;
            } else if self.mood != Mood::Chase {
                self.mood = Mood::Investigate;
            }
            self.remember(player.camera.position, map);
        } else if seconds - self.last_seen > CONTACT_GRACE {
            self.hate = (self.hate - HATE_DECAY * dt).hi(0.0);
        }
        if self.mood == Mood::Chase && self.hate <= CHASE_LEAVE {
            self.mood = Mood::Search;
            self.idle = 4.0;
        }
        let live = self.mood == Mood::Chase
            && (contact
                || self.hate >= LIVE_TRACK_MIN
                || seconds - self.last_seen <= CONTACT_GRACE);
        if live {
            self.remember(player.camera.position, map);
        }
        if self.mood != previous {
            self.path.clear();
            self.replan = 0.0;
        } else if self.target != previous_target {
            // Preserve the forward segment; returning to this cell's center on
            // avoids oscillation on moving-target updates.
            self.replan = self.replan.lo(0.15);
        }
        if self.attack_left > 0.0 {
            let elapsed = ATTACK_DURATION - self.attack_left;
            self.attack_left = (self.attack_left - dt).hi(0.0);
            let now = ATTACK_DURATION - self.attack_left;
            let direction = Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
            if elapsed < WINDUP_TIME {
                let d = player.camera.position - Vec3::new(self.position[0], 0.0, self.position[1]);
                turn_towards(&mut self.yaw, d.x.atan2(d.z), dt, 5.4);
            }
            // Commit to a direction without exceeding the normal chase speed.
            if distance > 0.72 {
                self.advance(
                    map,
                    direction,
                    dt * if elapsed < STRIKE_TIME {
                        CHASE_SPEED
                    } else {
                        3.2
                    },
                );
            }
            let mut damage = false;
            if !self.strike_done && elapsed < STRIKE_TIME && now >= STRIKE_TIME {
                self.strike_done = true;
                damage = self.reaches(map, player, 1.45, 0.25);
            }
            self.record_motion(before, dt);
            return damage;
        }
        let here = cell_at(self.position, map);
        if self.mood == Mood::Search && self.idle <= 0.0 {
            self.mood = Mood::Roam;
            self.path.clear();
        }
        if self.mood == Mood::Roam {
            if self.patrol_span == 0 {
                self.patrol_span = if self.patrol_anchor.is_some() { 8 } else { 24 };
            }
            // A smooth change in ordinary patrol geography, never a perception event.
            // In the opening minute its centre moves less than six metres, while
            // the entity still wanders, pauses and turns at its usual pace.
            self.patrol_age = (self.patrol_age + dt).lo(3600.0);
            let age2 = self.patrol_age * self.patrol_age;
            self.patrol_drift =
                (self.patrol_drift + dt * (0.08 + 0.40 * age2 / (age2 + 600.0 * 600.0))).lo(32.0);
        }
        if self.mood == Mood::Roam && self.path.is_empty() && self.idle <= 0.0 {
            let mut anchor = self.patrol_anchor.unwrap_or(here);
            let steps = (self.patrol_drift / map.tile_size) as usize;
            if steps > 0 {
                // Re-evaluate only when choosing an ordinary destination. Advance
                // through walkable cells, so a wall never attracts a patrol into it.
                let toward = path(
                    map,
                    anchor,
                    cell_at([player.camera.position.x, player.camera.position.z], map),
                );
                if let Some(&node) = toward.iter().take(steps).last() {
                    anchor = [node as usize % map.width(), node as usize / map.width()];
                }
                self.patrol_drift -= steps as f32 * map.tile_size;
            }
            self.patrol_anchor = Some(anchor);
            let radius =
                8 + ((self.patrol_span - 8) as f32 * 600.0 / (self.patrol_age + 600.0)) as u8;
            let candidates = patrol_cells(map, anchor, radius);
            self.target = if candidates.is_empty() {
                anchor
            } else {
                let node = candidates[self.random() as usize % candidates.len()] as usize;
                [node % map.width(), node / map.width()]
            };
            if clear_segment(map, self.position, cell_center(self.target, map)) {
                self.path
                    .push_front((self.target[1] * map.width() + self.target[0]) as u16);
            }
            self.replan = 0.0;
        }
        let goal = if self.mood == Mood::Roam {
            cell_center(self.target, map)
        } else {
            self.last_known
        };
        let direct = self.mood != Mood::Roam && clear_segment(map, self.position, goal);
        if direct {
            self.path.clear();
        } else if self.replan <= 0.0
            && self.target != here
            && !(self.mood == Mood::Roam && self.idle > 0.0)
            && (self.path.is_empty()
                || self.path.back()
                    != Some(&((self.target[1] * map.width() + self.target[0]) as u16)))
        {
            self.path = path(map, here, self.target);
            if let Some(&next) = self.path.front()
                && !clear_segment(map, self.position, node_center(next, map))
            {
                self.path
                    .push_front((here[1] * map.width() + here[0]) as u16);
            }
            self.replan = if self.mood == Mood::Chase { 0.20 } else { 1.2 };
        }
        let resting = self.mood == Mood::Roam && self.path.is_empty() && self.idle > 0.0;
        if !resting {
            let speed = match self.mood {
                Mood::Chase => 4.3 + (CHASE_SPEED - 4.3) * (self.hate / 100.0),
                Mood::Investigate => 2.1,
                _ => 1.15,
            };
            let turn = if self.mood == Mood::Chase { 1.7 } else { 1.9 };
            let mut remaining = dt;
            for _ in 0..4 {
                let waypoint = self.path.front().copied();
                let destination = waypoint.map_or(goal, |node| node_center(node, map));
                let stop = if waypoint.is_none() && self.mood == Mood::Chase {
                    0.70
                } else {
                    0.0
                };
                let d = norm2(
                    destination[0] - self.position[0],
                    destination[1] - self.position[1],
                );
                if d > stop + 0.02 {
                    remaining = self.walk_towards(map, destination, remaining, speed, turn, stop);
                }
                let arrived = norm2(
                    destination[0] - self.position[0],
                    destination[1] - self.position[1],
                ) <= stop + 0.03;
                if !arrived {
                    break;
                }
                if waypoint.is_some() {
                    self.path.pop_front();
                    if !self.path.is_empty() {
                        if remaining > 0.0001 {
                            continue;
                        }
                        break;
                    }
                }
                if self.mood == Mood::Roam {
                    self.idle = 1.5 + (self.random() % 3000) as u32 as f32 * 0.001;
                } else if self.mood == Mood::Investigate || (self.mood == Mood::Chase && !live) {
                    self.mood = Mood::Search;
                    self.idle = 4.0;
                }
                break;
            }
        } else {
            self.speed = (self.speed - dt * 20.0).hi(0.0);
            self.yaw = (self.yaw + dt * 0.10).rem_euclid(core::f32::consts::TAU);
        }
        if self.mood == Mood::Chase && self.cooldown <= 0.0 && self.reaches(map, player, 1.35, 0.20)
        {
            self.attack_left = ATTACK_DURATION;
            self.strike_done = false;
            self.cooldown = ATTACK_COOLDOWN;
        }
        self.record_motion(before, dt);
        false
    }
    pub fn draw(&self, map: &Map, mesh: &mut Mesh, time: f32) {
        let floor = map.floor_at(self.position[0], self.position[1]);
        draw_wire_pose(
            mesh,
            Vec3::new(self.position[0], floor, self.position[1]),
            self.yaw,
            time,
            self.form_seed,
            map.ceiling.min(3.5) / 3.5,
            Motion {
                travel: self.travel,
                movement: self.move_blend,
                attack_left: self.attack_left,
                rage: self.rage(),
            },
        );
    }
}
// Bounded patrol destinations are selected by walking distance, not a circle
// through walls. This remains cheap even when the map has long maze detours.
fn patrol_cells(map: &Map, anchor: [usize; 2], radius: u8) -> Vec<u16> {
    let width = map.width();
    let first = anchor[1] * width + anchor[0];
    let mut distance = vec![u8::MAX; width * map.height()];
    let mut queue = vec![first as u16];
    let mut candidates = Vec::new();
    distance[first] = 0;
    let mut at = 0;
    while at < queue.len() {
        let node = queue[at] as usize;
        at += 1;
        let d = distance[node];
        if d >= 2 {
            candidates.push(node as u16);
        }
        if d == radius {
            continue;
        }
        let x = (node % width) as i32;
        let z = (node / width) as i32;
        for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            if map.wall(x + dx, z + dz) {
                continue;
            }
            let next = (z + dz) as usize * width + (x + dx) as usize;
            if distance[next] != u8::MAX {
                continue;
            }
            #[cfg(any(feature = "tools", test))]
            {
                let p = cell_center([(x + dx) as usize, (z + dz) as usize], map);
                if !map.walkable(p[0], p[1], map.floor_at(p[0], p[1])) {
                    continue;
                }
            }
            distance[next] = d + 1;
            queue.push(next as u16);
        }
    }
    candidates
}
fn cell_at(position: [f32; 2], map: &Map) -> [usize; 2] {
    [
        (position[0] / map.tile_size) as u32 as usize,
        (position[1] / map.tile_size) as u32 as usize,
    ]
}
fn cell_center(cell: [usize; 2], map: &Map) -> [f32; 2] {
    [
        (cell[0] as u32 as f32 + 0.5) * map.tile_size,
        (cell[1] as u32 as f32 + 0.5) * map.tile_size,
    ]
}
fn node_center(node: u16, map: &Map) -> [f32; 2] {
    cell_center(
        [node as usize % map.width(), node as usize / map.width()],
        map,
    )
}
fn body_fits(map: &Map, position: [f32; 2], floor: f32) -> bool {
    // Pass the entity radius once; Map::walkable already includes the player radius.
    map.walkable_radius(position[0], position[1], floor, BODY_RADIUS)
}
#[inline(never)]
fn clear_segment(map: &Map, from: [f32; 2], to: [f32; 2]) -> bool {
    // Sampling every <=5 cm with 3 cm extra clearance conservatively covers the
    // gaps between samples can miss a grazing corner.
    let dx = to[0] - from[0];
    let dz = to[1] - from[1];
    let count = (norm2(dx, dz) / 0.05) as u32 + 1;
    let floor = map.floor_at(from[0], from[1]);
    for i in 0..=count {
        let t = i as f32 / count as f32;
        if !map.walkable_radius(
            from[0] + dx * t,
            from[1] + dz * t,
            floor,
            BODY_RADIUS + 0.03,
        ) {
            return false;
        }
    }
    true
}
// One breadth-first traversal serves both path planning and bounded hearing.
#[inline(never)]
fn search(
    map: &Map,
    start: [usize; 2],
    goal: [usize; 2],
    limit: usize,
    walking: bool,
) -> Option<Vec<u32>> {
    #[cfg(not(any(feature = "tools", test)))]
    let _ = walking;
    let w = map.width();
    let count = w * map.height();
    assert!(count <= 65536);
    if start[0] >= w
        || start[1] >= map.height()
        || goal[0] >= w
        || goal[1] >= map.height()
        || map.wall(goal[0] as i32, goal[1] as i32)
    {
        return None;
    }
    let first = start[1] * w + start[0];
    let end = goal[1] * w + goal[0];
    // u32 parents keep cell 65535 distinct from the unvisited sentinel.
    // Every cell is enqueued once, so a fixed count-sized tail is sufficient.
    let mut storage = vec![u32::MAX; count * 2];
    let parent = storage.as_mut_ptr();
    let queue = unsafe { parent.add(count) };
    unsafe {
        *parent.add(first) = first as u32;
        *queue = first as u32;
    }
    let mut head = 0;
    let mut tail = 1;
    for _ in 0..=limit {
        let end_layer = tail;
        if head == tail {
            break;
        }
        while head < end_layer {
            let i = unsafe { *queue.add(head) as usize };
            head += 1;
            if i == end {
                return Some(storage);
            }
            for offset in [-(w as isize), 1, w as isize, -1] {
                let next = i.wrapping_add_signed(offset);
                if next >= count {
                    continue;
                }
                #[cfg(any(feature = "tools", test))]
                if offset.abs() == 1 && i / w != next / w {
                    continue;
                }
                // Runtime maps have a solid outer border, so open nodes cannot
                // wrap across rows. The bounded index addresses a valid wall bit.
                if unsafe { *map.walls.bits.as_ptr().add(next >> 6) } & (1 << (next & 63)) != 0 {
                    continue;
                }
                if unsafe { *parent.add(next) } != u32::MAX {
                    continue;
                }
                // Runtime maps contain only flat, full-width tile corridors.
                #[cfg(any(feature = "tools", test))]
                if walking {
                    let p = cell_center([next % w, next / w], map);
                    if !map.walkable(p[0], p[1], map.floor_at(p[0], p[1])) {
                        continue;
                    }
                }
                unsafe {
                    *parent.add(next) = i as u32;
                    *queue.add(tail) = next as u32;
                }
                tail += 1;
            }
        }
    }
    None
}
pub fn path(map: &Map, start: [usize; 2], goal: [usize; 2]) -> Route {
    let mut out = Route::new();
    if let Some(parent) = search(map, start, goal, map.width() * map.height(), true) {
        let first = start[1] * map.width() + start[0];
        let mut at = goal[1] * map.width() + goal[0];
        while at != first {
            out.push_front(at as u16);
            at = parent[at] as usize;
        }
    }
    out
}
#[inline(never)]
fn turn_towards(yaw: &mut f32, target: f32, dt: f32, speed: f32) {
    let delta = (target - *yaw + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU)
        - core::f32::consts::PI;
    *yaw = (*yaw + delta.bound(-speed * dt, speed * dt)).rem_euclid(core::f32::consts::TAU);
}
fn audible(map: &Map, origin: [f32; 2], listener: Vec3, radius: f32) -> bool {
    search(
        map,
        cell_at(origin, map),
        cell_at([listener.x, listener.z], map),
        ((radius + 0.15) / map.tile_size) as u32 as usize,
        false,
    )
    .is_some()
}
#[derive(Clone, Copy, Debug)]
pub struct LimbPose {
    pub root: Vec3,
    pub joint: Vec3,
    pub tip: Vec3,
}
#[derive(Clone, Copy, Debug)]
pub struct SkeletonPose {
    pub pelvis: Vec3,
    pub chest: Vec3,
    pub neck: Vec3,
    pub head: Vec3,
    pub legs: [LimbPose; 2],
    pub arms: [LimbPose; 2],
}
#[derive(Clone, Copy, Default)]
pub struct Motion {
    pub travel: f32,
    pub movement: f32,
    pub attack_left: f32,
    pub rage: f32,
}
#[allow(dead_code)]
mod program {
    include!("entity_program.rs");
}
// Only the checked-in program emitted by compile_entity.py enters this VM.
// The compiler proves register indices, rig indices, branch stack heights,
// operand stack depth <= 32 and non-recursive call depth <= 8.
struct Formula {
    registers: [Vec3; program::REGISTERS],
}
impl Formula {
    fn new() -> Self {
        Self {
            registers: [Vec3::default(); program::REGISTERS],
        }
    }
    fn run(&mut self, mesh: &mut Mesh, function: usize, args: &[Vec3]) {
        use program::*;
        let Function { entry, base, count } = FUNCTIONS[function];
        self.registers[base as usize..base as usize + count as usize].copy_from_slice(args);
        #[cfg(target_arch = "x86_64")]
        {
            unsafe {
                formula_eval(
                    self.registers.as_mut_ptr().cast(),
                    CODE.as_ptr().add(entry as usize),
                    (mesh as *mut Mesh).cast(),
                );
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        self.interpret(mesh, entry as usize);
    }
    #[cfg(not(target_arch = "x86_64"))]
    fn interpret(&mut self, mesh: &mut Mesh, entry: usize) {
        use program::*;
        let mut pc = entry;
        let mut stack = [Vec3::default(); 32];
        let mut sp = 0;
        let mut calls = [0usize; 8];
        let mut cp = 0;
        macro_rules! pop {
            () => {{
                sp -= 1;
                unsafe { *stack.as_ptr().add(sp) }
            }};
        }
        let r = self.registers.as_mut_ptr();
        let byte = |pc: usize| unsafe { *CODE.as_ptr().add(pc) };
        loop {
            let op = byte(pc);
            pc += 1;
            let value = if op >= 48 + REGISTERS as u8 {
                splat(CONSTANTS[(op - 48 - REGISTERS as u8) as usize])
            } else if op >= 48 {
                self.registers[(op - 48) as usize]
            } else {
                match op {
                    SET => {
                        let v = pop!();
                        unsafe {
                            *r.add(byte(pc) as usize) = v;
                        }
                        pc += 1;
                        continue;
                    }
                    INDEX => {
                        let i = pop!().x as usize;
                        unsafe { *r.add(i) }
                    }
                    PUT => {
                        let v = pop!();
                        let i = pop!().x as usize;
                        unsafe {
                            *r.add(i) = v;
                        }
                        continue;
                    }
                    JUMP | ZERO => {
                        let target = u16::from_le_bytes([byte(pc), byte(pc + 1)]) as u32 as usize;
                        pc += 2;
                        if op == JUMP || pop!().x == 0.0 {
                            pc = target;
                        }
                        continue;
                    }
                    CALL => {
                        let Function { entry, base, count } =
                            unsafe { *FUNCTIONS.as_ptr().add(byte(pc) as usize) };
                        pc += 1;
                        sp -= count as usize;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                stack.as_ptr().add(sp),
                                r.add(base as usize),
                                count as usize,
                            );
                        }
                        unsafe {
                            *calls.as_mut_ptr().add(cp) = pc;
                        }
                        cp += 1;
                        pc = entry as usize;
                        continue;
                    }
                    RETURN => {
                        if cp == 0 {
                            return;
                        }
                        cp -= 1;
                        pc = unsafe { *calls.as_ptr().add(cp) };
                        continue;
                    }
                    POP => {
                        let _ = pop!();
                        continue;
                    }
                    QUAD | RQUAD => {
                        let d = pop!();
                        let c = pop!();
                        let b = pop!();
                        let a = pop!();
                        mesh.add_quad(
                            [a, b, c, d],
                            if op == QUAD {
                                Material::Wire
                            } else {
                                Material::Rage
                            },
                        );
                        continue;
                    }
                    VEC => {
                        let z = pop!().x;
                        let y = pop!().x;
                        let x = pop!().x;
                        Vec3::new(x, y, z)
                    }

                    _ => {
                        let b = pop!();
                        match op {
                            NEG => b * -1.0,
                            SIN => splat(b.x.sin()),
                            COS => splat(b.x.cos()),
                            ABS => splat(b.x.abs()),
                            SQRT => splat(b.x.sqrt()),
                            _ => {
                                let a = pop!();
                                match op {
                                    ADD => a + b,
                                    SUB => a - b,
                                    MUL => a * b.x,
                                    DIV => Vec3::new(a.x / b.x, a.y / b.x, a.z / b.x),
                                    LT => splat((a.x < b.x) as u8 as f32),
                                    EQ => splat((a.x == b.x) as u8 as f32),
                                    POW => splat(a.x.powf(b.x)),
                                    REM => splat(a.x.rem_euclid(b.x)),
                                    MIN => splat(a.x.lo(b.x)),
                                    MAX => splat(a.x.hi(b.x)),
                                    DOT => splat(a.dot(b)),
                                    CROSS => a.cross(b),
                                    _ => unreachable!(),
                                }
                            }
                        }
                    }
                }
            };
            unsafe {
                *stack.as_mut_ptr().add(sp) = value;
            }
            sp += 1;
        }
    }
}
fn splat(x: f32) -> Vec3 {
    Vec3::new(x, x, x)
}
#[inline(never)]
pub fn skeleton(travel: f32, movement: f32, attack_left: f32, scale: f32) -> SkeletonPose {
    let mut f = Formula::new();
    f.run(
        &mut Mesh::default(),
        program::FN_SKELETON,
        &[
            splat(travel),
            splat(movement),
            splat(attack_left),
            splat(scale),
        ],
    );
    let r = f.registers;
    let limb = |i| LimbPose {
        root: r[i],
        joint: r[i + 1],
        tip: r[i + 2],
    };
    SkeletonPose {
        pelvis: r[0],
        chest: r[1],
        neck: r[2],
        head: r[3],
        legs: [limb(4), limb(7)],
        arms: [limb(10), limb(13)],
    }
}
pub fn draw_wire(mesh: &mut Mesh, position: Vec3, yaw: f32, time: f32, seed: u64, scale: f32) {
    draw_wire_pose(mesh, position, yaw, time, seed, scale, Motion::default());
}
#[inline(never)]
pub fn draw_wire_pose(
    mesh: &mut Mesh,
    position: Vec3,
    yaw: f32,
    time: f32,
    seed: u64,
    scale: f32,
    motion: Motion,
) {
    Formula::new().run(
        mesh,
        program::FN_DRAW,
        &[
            position,
            splat(yaw),
            splat(time),
            splat((seed % 997) as f32),
            splat(scale),
            splat(motion.travel),
            splat(motion.movement),
            splat(motion.attack_left),
            splat(motion.rage),
        ],
    );
}

// Simulation inputs are finite; avoid the generic NaN-preserving min/max path.
trait FiniteFloat {
    fn lo(self, b: Self) -> Self;
    fn hi(self, b: Self) -> Self;
    fn bound(self, a: Self, b: Self) -> Self
    where
        Self: Sized,
    {
        self.hi(a).lo(b)
    }
}
impl FiniteFloat for f32 {
    fn lo(self, b: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use core::arch::x86_64::*;
            _mm_cvtss_f32(_mm_min_ss(_mm_set_ss(self), _mm_set_ss(b)))
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            self.min(b)
        }
    }
    fn hi(self, b: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use core::arch::x86_64::*;
            _mm_cvtss_f32(_mm_max_ss(_mm_set_ss(self), _mm_set_ss(b)))
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            self.max(b)
        }
    }
}

#[cfg(target_arch = "x86_64")]
unsafe extern "win64" {
    fn halls_formula(
        registers: *mut core::ffi::c_void,
        entry: *const u8,
        mesh: *mut core::ffi::c_void,
    );
}
#[cfg(target_arch = "x86_64")]
use halls_formula as formula_eval;
#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(include_str!(concat!(env!("OUT_DIR"),"/entity_vm.s")),
    entry=sym halls_formula, code=sym program::CODE, constants=sym program::CONSTANTS,
    functions=sym program::FUNCTIONS, bridge=sym formula_bridge,
    constant_base=const 48+program::REGISTERS);
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct NativeFormula {
    mesh: *mut Mesh,
}
#[cfg(target_arch = "x86_64")]
unsafe extern "win64" fn formula_bridge(
    ctx: *mut NativeFormula,
    mut stack: *mut Vec3,
    op: u32,
) -> *mut Vec3 {
    use program::*;
    // Only the statically validated program can reach this internal ABI.
    unsafe {
        macro_rules! pop {
            () => {{
                stack = stack.sub(1);
                *stack
            }};
        }
        let value = match op as u8 {
            QUAD | RQUAD => {
                let d = pop!();
                let c = pop!();
                let b = pop!();
                let a = pop!();
                (*(*ctx).mesh).add_quad(
                    [a, b, c, d],
                    if op as u8 == QUAD {
                        Material::Wire
                    } else {
                        Material::Rage
                    },
                );
                return stack;
            }
            POW => {
                let b = pop!().x;
                let a = pop!().x;
                splat(a.powf(b))
            }
            REM => {
                let b = pop!().x;
                let a = pop!().x;
                splat(a.rem_euclid(b))
            }
            _ => core::hint::unreachable_unchecked(),
        };
        *stack = value;
        stack.add(1)
    }
}

fn norm2(x: f32, z: f32) -> f32 {
    let d = x * x + z * z;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::*;
        _mm_cvtss_f32(_mm_sqrt_ss(_mm_set_ss(d)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        d.sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exit_reuses_exactly_the_enraged_entity_particle_geometry() {
        let mut full = Mesh::default();
        let mut cloud = Mesh::default();
        for (mesh, scale) in [(&mut full, 0.75), (&mut cloud, -0.75)] {
            draw_wire_pose(
                mesh,
                Vec3::new(5.0, 0.0, 7.0),
                0.0,
                2.0,
                0,
                scale,
                Motion {
                    rage: 1.0,
                    ..Default::default()
                },
            );
        }
        let particles: Vec<_> = full
            .triangles
            .iter()
            .filter(|t| t.material == Material::Rage)
            .collect();
        assert_eq!(cloud.triangles.len(), 36 * 4);
        assert_eq!(particles.len(), cloud.triangles.len());
        for (a, b) in particles.iter().zip(&cloud.triangles) {
            assert_eq!(b.material, Material::Rage);
            for (a, b) in a.vertices.iter().zip(&b.vertices) {
                assert_eq!(a.p, b.p);
            }
        }
    }
    #[test]
    fn highest_u16_cell_is_not_confused_with_unvisited() {
        let mut map = Map::procedural(42).unwrap();
        for z in 254..256 {
            for x in 254..256 {
                map.set_wall(x, z, false);
            }
        }
        let route = path(&map, [254, 254], [255, 255]);
        assert_eq!(route.len(), 2);
        assert_eq!(route.back(), Some(&u16::MAX));
    }
    #[test]
    fn route_goes_around_a_wall() {
        let map = Map::generated(7, 1).unwrap();
        let route = path(&map, [1, 1], map.exit);
        assert!(!route.is_empty());
        assert!(route.iter().all(|&n| !map.wall(
            (n as usize % map.width()) as i32,
            (n as usize / map.width()) as i32
        )));
    }
    #[test]
    fn wall_occludes_detection() {
        let mut map = Map::generated(7, 1).unwrap();
        map.set_wall(2, 1, true);
        let mut p = Player::new(&map);
        p.camera.position.x = 7.0;
        let mut e = Entity::at_cell(&map, [1, 1], 1);
        e.position = [3.0, 3.0];
        e.yaw = core::f32::consts::FRAC_PI_2;
        e.replan = 9.0;
        e.idle = 9.0;
        let seen = e.tick(&map, &p, 10.0, 0.02, 0.0, true);
        assert!(!seen);
        assert_ne!(e.mood, Mood::Chase);
        assert_ne!(e.mood, Mood::Chase);
    }
    #[test]
    fn sight_triggers_chase_and_close_contact_attacks() {
        let map = Map::generated(7, 1).unwrap();
        let p = Player::new(&map);
        let mut e = Entity::at_cell(&map, [1, 1], 1);
        e.position = [3.0, 3.9];
        e.yaw = core::f32::consts::PI;
        let encounter = e.tick(&map, &p, 10.0, 0.02, 0.0, true);
        assert!(!encounter);
        assert_eq!(e.mood, Mood::Chase);
        let mut strikes = 0;
        for i in 1..70 {
            strikes +=
                usize::from(e.tick(&map, &p, 10.0 + i as u32 as f32 * 0.02, 0.02, 0.0, true));
        }
        assert_eq!(strikes, 2);
    }
    #[test]
    fn chase_reaches_a_player_off_the_cell_center() {
        let map = Map::generated(9, 1).unwrap();
        let mut player = Player::new(&map);
        player.camera.position.z = 3.25;
        let mut e = Entity::at_cell(&map, [1, 1], 1);
        e.position = [3.0, 5.75];
        e.yaw = core::f32::consts::PI;
        e.target = [1, 1];
        let mut attacked = false;
        for i in 0..120 {
            attacked |= e.tick(
                &map,
                &player,
                10.0 + i as u32 as f32 * 0.02,
                0.02,
                0.0,
                true,
            );
        }
        assert!(attacked);
    }
    fn open_room(size: usize) -> Map {
        let mut m = Map::generated(size, 1).unwrap();
        for z in 1..size - 1 {
            for x in 1..size - 1 {
                m.set_wall(x, z, false);
            }
        }
        m.spawn.x = size as f32;
        m.spawn.z = size as f32;
        m.exit = [size - 2, size - 2];
        m
    }
    #[test]
    fn walking_is_quiet_but_running_can_be_heard_further() {
        let m = open_room(65);
        let p = Player::new(&m);
        let make = |distance: f32| {
            let mut e = Entity::at_cell(&m, [8, 8], 1);
            e.position = [p.camera.position.x, p.camera.position.z + distance];
            e.yaw = 0.0;
            e.idle = 10.0;
            e.replan = 10.0;
            e
        };
        let mut far_walk = make(7.0);
        far_walk.tick(&m, &p, 10.0, 0.02, HEARING_WALK, true);
        assert_eq!(far_walk.mood, Mood::Roam);
        let mut nearby = make(2.7);
        nearby.tick(&m, &p, 10.0, 0.02, HEARING_WALK, true);
        assert_eq!(nearby.mood, Mood::Investigate);
        let mut far_run = make(28.0);
        far_run.tick(&m, &p, 10.0, 0.02, HEARING_RUN, true);
        assert_eq!(far_run.mood, Mood::Investigate);
        let mut beyond_run = make(36.0);
        beyond_run.tick(&m, &p, 10.0, 0.02, HEARING_RUN, true);
        assert_eq!(beyond_run.mood, Mood::Roam);
    }
    #[test]
    fn hearing_cannot_locate_a_player_through_a_sealed_wall() {
        let mut m = open_room(17);
        for x in 1..16 {
            m.set_wall(x, 8, true);
        }
        let origin = [17.0, 15.0];
        let p = Vec3::new(17.0, 1.55, 19.0);
        assert!(!audible(&m, origin, p, HEARING_RUN));
    }
    #[test]
    fn movement_keeps_a_stance_foot_planted_and_lifts_the_swing_foot() {
        let a = skeleton(0.20, 1.0, 0.0, 1.0);
        let b = skeleton(0.25, 1.0, 0.0, 1.0);
        assert!((0.20 + a.legs[0].tip.z - 0.25 - b.legs[0].tip.z).abs() < 0.0001);
        assert!((a.legs[0].tip.y - 0.035).abs() < 0.0001);
        assert!(a.legs[1].tip.y > 0.05);
        assert!((a.legs[0].joint - a.legs[0].root).length() > 0.70);
    }
    #[test]
    fn attack_has_a_windup_and_can_miss_an_escaping_player() {
        let m = open_room(17);
        let mut p = Player::new(&m);
        let mut e = Entity::at_cell(&m, [8, 8], 1);
        e.position[1] += 0.8;
        e.yaw = core::f32::consts::PI;
        assert!(!e.tick(&m, &p, 10.0, 0.02, 0.0, true));
        assert!(e.attack_left > 0.0);
        p.camera.position.z -= 2.5;
        let mut hit = false;
        for i in 1..50 {
            hit |= e.tick(&m, &p, 10.0 + i as u32 as f32 * 0.02, 0.02, 0.0, true);
        }
        assert!(!hit);
        let idle = skeleton(0.0, 0.0, 0.0, 1.0);
        let windup = skeleton(0.0, 0.0, ATTACK_DURATION - 0.05, 1.0);
        let strike = skeleton(0.0, 0.0, ATTACK_DURATION - STRIKE_TIME, 1.0);
        assert!(windup.arms[1].tip.z < idle.arms[1].tip.z - 0.25);
        assert!(strike.arms[1].tip.z > 0.9);
    }
    #[test]
    fn roaming_pauses_briefly_then_travels_without_player_noise() {
        let mut m = open_room(65);
        for z in 1..64 {
            m.set_wall(32, z, true);
        }
        m.entities = vec![[8, 8], [24, 50], [10, 40]];
        let mut player = Player::new(&m);
        player.camera.position.x = 97.0;
        let mut enemy = Entity::at_cell(&m, [8, 8], 42);
        enemy.idle = 2.0;
        let start = enemy.position;
        for i in 0..20 {
            enemy.tick(&m, &player, 10.0 + i as u32 as f32 * 0.05, 0.05, 0.0, true);
        }
        assert_eq!(enemy.position, start);
        let mut longest_pause = 0.0f32;
        let mut pause = 0.0f32;
        let mut cells = std::collections::HashSet::new();
        for i in 20..2400 {
            let before = enemy.position;
            enemy.tick(&m, &player, 10.0 + i as u32 as f32 * 0.05, 0.05, 0.0, true);
            assert_eq!(enemy.mood, Mood::Roam);
            assert!(m.walkable(enemy.position[0], enemy.position[1], 0.0));
            if enemy.position == before {
                pause += 0.05;
                longest_pause = longest_pause.max(pause);
            } else {
                pause = 0.0;
            }
            cells.insert([
                (enemy.position[0] / m.tile_size) as usize,
                (enemy.position[1] / m.tile_size) as usize,
            ]);
        }
        assert!(
            enemy.travel > 70.0,
            "entity failed to patrol: {}",
            enemy.travel
        );
        assert!(cells.len() > 25);
        assert!(
            longest_pause < 6.0,
            "entity remained idle for {longest_pause}"
        );
    }
    fn locked(map: &Map, cell: [usize; 2], player: &Player) -> Entity {
        let mut enemy = Entity::at_cell(map, cell, 42);
        enemy.mood = Mood::Chase;
        enemy.hate = 100.0;
        enemy.last_seen = 10.0;
        enemy.speed = CHASE_SPEED;
        enemy.remember(player.camera.position, map);
        enemy.yaw = (player.camera.position.x - enemy.position[0])
            .atan2(player.camera.position.z - enemy.position[1]);
        enemy
    }
    #[test]
    fn hate_rises_fast_retains_a_distant_target_then_decays_slowly() {
        let m = open_room(129);
        let mut player = Player::new(&m);
        player.camera.position.z += 8.0;
        let mut enemy = Entity::at_cell(&m, [64, 64], 42);
        enemy.mood = Mood::Chase;
        enemy.hate = CHASE_ENTER;
        enemy.last_seen = 10.0;
        for i in 0..25 {
            enemy.tick(&m, &player, 10.0 + i as u32 as f32 * 0.05, 0.05, 0.0, true);
        }
        assert_eq!(enemy.hate, 100.0);
        player.camera.position.x = 240.0;
        player.camera.position.z = 240.0;
        let lost_at = enemy.last_seen;
        for i in 1..=70 {
            enemy.tick(
                &m,
                &player,
                lost_at + i as u32 as f32 * 0.05,
                0.05,
                0.0,
                true,
            );
        }
        assert_eq!(enemy.hate, 100.0);
        assert_eq!(enemy.target, cell_at([240.0, 240.0], &m));
        assert_eq!(enemy.mood, Mood::Chase);
        for i in 71..=140 {
            enemy.tick(
                &m,
                &player,
                lost_at + i as u32 as f32 * 0.05,
                0.05,
                0.0,
                true,
            );
        }
        assert!((enemy.hate - 82.0).abs() < 0.6, "hate {}", enemy.hate);
        for i in 141..=400 {
            enemy.tick(
                &m,
                &player,
                lost_at + i as u32 as f32 * 0.05,
                0.05,
                0.0,
                true,
            );
        }
        assert!(enemy.hate < CHASE_LEAVE);
        assert_ne!(enemy.mood, Mood::Chase);
    }
    #[test]
    fn one_enraged_entity_does_not_alert_others() {
        let m = open_room(65);
        let player = Player::new(&m);
        let mut enemies = [
            locked(&m, [32, 35], &player),
            Entity::at_cell(&m, [52, 52], 11),
            Entity::at_cell(&m, [12, 12], 17),
        ];
        for i in 0..100 {
            for enemy in &mut enemies {
                enemy.tick(&m, &player, 10.0 + i as u32 as f32 * 0.05, 0.05, 0.0, true);
            }
        }
        assert_eq!(enemies[0].hate, 100.0);
        for enemy in &enemies[1..] {
            assert_eq!(enemy.hate, 0.0);
            assert_eq!(enemy.mood, Mood::Roam);
        }
    }
    #[test]
    fn a_locked_entity_catches_straight_sprinting_and_open_room_circling() {
        let m = open_room(129);
        let mut player = Player::new(&m);
        player.camera.position = Vec3::new(41.0, m.eye_height, 47.0);
        player.camera.yaw = 0.0;
        let mut enemy = locked(&m, [20, 20], &player);
        let mut caught_at = None;
        for i in 0..1800 {
            player.move_by(&m, crate::player::RUN_SPEED / 60.0, 0.0);
            if enemy.tick(
                &m,
                &player,
                10.0 + i as u32 as f32 / 60.0,
                1.0 / 60.0,
                0.0,
                true,
            ) {
                caught_at = Some(i as u32 as f32 / 60.0);
                break;
            }
        }
        assert!(
            caught_at.is_some_and(|t| (2.5..5.5).contains(&t)),
            "straight chase {caught_at:?}"
        );
        println!(
            "Straight sprint, 6m lead: caught after {:.2}s; chase cap {:.3}m/s",
            caught_at.unwrap(),
            CHASE_SPEED
        );
        player.camera.position = Vec3::new(129.0, m.eye_height, 134.0);
        let mut enemy = locked(&m, [64, 64], &player);
        let mut struck = false;
        for i in 0..900 {
            let angle = i as u32 as f32 / 60.0 * (crate::player::RUN_SPEED / 5.0);
            let goal = Vec3::new(
                129.0 + angle.sin() * 5.0,
                m.eye_height,
                129.0 + angle.cos() * 5.0,
            );
            let d = goal - player.camera.position;
            player.camera.yaw = d.x.atan2(d.z);
            player.move_by(&m, d.length(), 0.0);
            struck |= enemy.tick(
                &m,
                &player,
                10.0 + i as u32 as f32 / 60.0,
                1.0 / 60.0,
                0.0,
                true,
            );
            if struck {
                break;
            }
        }
        assert!(struck, "circling remained safe");
    }
    #[test]
    fn repeated_tight_corners_remain_pursuable_without_wall_clipping() {
        let mut m = Map::generated(129, 42).unwrap();
        for z in 1..128 {
            for x in 1..128 {
                m.set_wall(x, z, true);
            }
        }
        let mut goals = Vec::new();
        for row in 0..54 {
            let z = 3 + row * 2;
            for x in 4..=10 {
                m.set_wall(x, z, false);
            }
            let x = if row % 2 == 0 { 10 } else { 4 };
            if row < 53 {
                m.set_wall(x, z + 1, false);
            }
            goals.push(cell_center([x, z], &m));
            if row < 53 {
                goals.push(cell_center([x, z + 2], &m));
            }
        }
        m.spawn.x = 15.0;
        m.spawn.z = 7.0;
        m.spawn.yaw = core::f32::consts::FRAC_PI_2;
        m.exit = [4, 109];
        let mut player = Player::new(&m);
        let mut enemy = locked(&m, [4, 3], &player);
        let mut goal_index = 0;
        let mut foot = 0.0;
        let mut caught = false;
        for i in 0..6000 {
            if goal_index >= goals.len() {
                break;
            }
            let goal = goals[goal_index];
            let d = Vec3::new(
                goal[0] - player.camera.position.x,
                0.0,
                goal[1] - player.camera.position.z,
            );
            if d.length() < 0.02 {
                goal_index += 1;
                continue;
            }
            player.camera.yaw = d.x.atan2(d.z);
            let step = (crate::player::RUN_SPEED / 60.0).min(d.length());
            player.move_by(&m, step, 0.0);
            foot += step;
            let noise = if foot >= 0.80 {
                foot -= 0.80;
                HEARING_RUN
            } else {
                0.0
            };
            caught |= enemy.tick(
                &m,
                &player,
                10.0 + i as u32 as f32 / 60.0,
                1.0 / 60.0,
                noise,
                true,
            );
            assert!(body_fits(&m, enemy.position, 0.0));
            if caught {
                break;
            }
        }
        assert!(caught, "the faster pursuer stalled at repeated corners");
        assert!(goal_index >= 2, "course did not exercise a corner");
    }
    #[test]
    fn fury_adds_particles_and_keeps_appearance_independent_of_patrol_rng() {
        let m = open_room(17);
        let mut enemy = Entity::at_cell(&m, [8, 8], 42);
        let mut normal = Mesh::default();
        enemy.draw(&m, &mut normal, 10.0);
        assert!(
            normal
                .triangles
                .iter()
                .all(|t| t.material != Material::Rage)
        );
        enemy.random();
        enemy.random();
        let mut after_rng = Mesh::default();
        enemy.draw(&m, &mut after_rng, 10.0);
        assert_eq!(normal.triangles.len(), after_rng.triangles.len());
        for (a, b) in normal.triangles.iter().zip(&after_rng.triangles) {
            assert_eq!(a.vertices.map(|v| v.p), b.vertices.map(|v| v.p));
        }
        enemy.hate = 100.0;
        let mut furious = Mesh::default();
        enemy.draw(&m, &mut furious, 10.0);
        assert_eq!(
            furious
                .triangles
                .iter()
                .filter(|t| t.material == Material::Rage)
                .count(),
            144
        );
        assert!(
            furious
                .triangles
                .iter()
                .flat_map(|t| t.vertices)
                .all(|v| v.p.x.is_finite() && v.p.y.is_finite() && v.p.z.is_finite())
        );
    }
    #[test]
    fn grazing_corners_do_not_trap_a_pursuer() {
        let mut m = open_room(25);
        for z in 6..19 {
            m.set_wall(12, z, true);
        }
        for offset in [-0.7f32, -0.35, 0.0, 0.35, 0.7] {
            let mut player = Player::new(&m);
            player.camera.position = Vec3::new(28.0 + offset, m.eye_height, 15.0);
            let mut enemy = locked(&m, [10, 8], &player);
            enemy.position[0] += offset;
            enemy.position[1] += 0.4;
            let mut hit = false;
            let mut stationary = 0;
            let mut longest = 0;
            for frame in 0..1800 {
                let before = enemy.position;
                hit |= enemy.tick(
                    &m,
                    &player,
                    10.0 + frame as f32 / 60.0,
                    1.0 / 60.0,
                    HEARING_RUN,
                    true,
                );
                if hit {
                    break;
                }
                if (before[0] - enemy.position[0]).hypot(before[1] - enemy.position[1]) < 0.0001 {
                    stationary += 1;
                } else {
                    stationary = 0;
                }
                longest = longest.max(stationary);
                assert!(body_fits(&m, enemy.position, 0.0));
            }
            assert!(
                hit,
                "stuck at {:?}, offset {offset}, longest stop {longest}",
                enemy.position
            );
            assert!(
                longest < 60,
                "corner stopped entity for {}s",
                longest as f32 / 60.0
            );
        }
    }
    #[test]
    fn turns_keep_three_quarters_of_the_former_speed_loss_and_remain_rate_limited() {
        let map = open_room(33);
        for angle in [0.0f32, 0.35, 0.8, core::f32::consts::FRAC_PI_2] {
            let mut enemy = Entity::at_cell(&map, [10, 10], 1);
            enemy.speed = CHASE_SPEED;
            enemy.yaw = angle;
            let goal = [enemy.position[0], enemy.position[1] + 20.0];
            enemy.walk_towards(&map, goal, 1.0, CHASE_SPEED, 0.0, 0.0);
            let old_loss = CHASE_SPEED * 0.84 * (1.0 - angle.cos().max(0.0).powi(4));
            assert!(((CHASE_SPEED - enemy.speed) - old_loss * 0.75).abs() < 0.00001);
        }
        let mut enemy = Entity::at_cell(&map, [10, 10], 1);
        enemy.speed = CHASE_SPEED;
        let old_yaw = enemy.yaw;
        let before = enemy.position;
        let goal = [before[0] + 20.0, before[1]];
        enemy.walk_towards(&map, goal, 1.0 / 60.0, CHASE_SPEED, 1.7, 0.0);
        assert!((enemy.yaw - old_yaw).abs() <= 1.7 / 60.0 + 0.00001);
        assert!(
            (before[0] - enemy.position[0]).hypot(before[1] - enemy.position[1])
                <= CHASE_SPEED / 60.0 + 0.00001
        );
        let mut blocked = map.clone();
        blocked.set_wall(11, 10, true);
        let mut enemy = Entity::at_cell(&blocked, [10, 10], 1);
        enemy.speed = CHASE_SPEED;
        enemy.advance(&blocked, Vec3::new(1.0, 0.0, 0.0), 2.0);
        assert!((enemy.speed - CHASE_SPEED * 0.625).abs() < 0.00001);
        assert!(body_fits(&blocked, enemy.position, 0.0));
    }
    #[test]
    fn the_central_patrol_stays_local_and_returns_after_a_chase() {
        let map = open_room(129);
        let mut player = Player::new(&map);
        player.camera.position.x = -500.0;
        player.camera.position.z = -500.0;
        let mut enemy = Entity::at_cell(&map, [64, 64], 42);
        enemy.patrol_anchor = Some([64, 64]);
        for frame in 0..2400 {
            enemy.tick(&map, &player, frame as f32 / 20.0, 0.05, 0.0, false);
            let distance = (enemy.position[0] - 129.0).abs() + (enemy.position[1] - 129.0).abs();
            assert!(
                distance <= 16.1,
                "patrol wandered {distance}m from its route"
            );
        }
        assert!(enemy.travel > 70.0);
        enemy.position[0] += 40.0;
        enemy.mood = Mood::Roam;
        enemy.path.clear();
        enemy.idle = 0.0;
        for frame in 0..1200 {
            enemy.tick(&map, &player, 120.0 + frame as f32 / 20.0, 0.05, 0.0, false);
        }
        assert!((enemy.position[0] - 129.0).abs() + (enemy.position[1] - 129.0).abs() <= 16.1);
    }
    #[test]
    fn patrol_drift_remains_weak_in_the_opening_minutes() {
        let map = open_room(256);
        let mut player = Player::new(&map);
        player.camera.position.x = 449.0;
        player.camera.position.z = 449.0;
        let mut enemy = Entity::at_cell(&map, [30, 30], 42);
        for frame in 0..6000 {
            enemy.tick(&map, &player, frame as f32 * 0.05, 0.05, 0.0, false);
            assert_eq!(enemy.mood, Mood::Roam);
            assert_eq!(enemy.hate, 0.0);
            if frame == 1199 || frame == 5999 {
                let anchor = enemy.patrol_anchor.unwrap();
                let drift =
                    (anchor[0].abs_diff(30) + anchor[1].abs_diff(30)) as f32 * map.tile_size;
                assert!(
                    drift <= if frame == 1199 { 6.0 } else { 34.0 },
                    "{drift}m drift"
                );
            }
        }
        assert!(enemy.travel > 250.0);
    }
    #[test]
    fn remote_player_changes_do_not_retarget_an_active_patrol() {
        let map = open_room(256);
        let mut player = Player::new(&map);
        player.camera.position.x = 449.0;
        player.camera.position.z = 449.0;
        let mut enemy = Entity::at_cell(&map, [30, 30], 42);
        enemy.tick(&map, &player, 100.0, 0.05, 0.0, true);
        let target = enemy.target;
        let anchor = enemy.patrol_anchor;
        player.camera.position.x = 101.0;
        for frame in 0..10 {
            enemy.tick(&map, &player, 100.05 + frame as f32 * 0.05, 0.05, 0.0, true);
            assert_eq!(enemy.target, target);
            assert_eq!(enemy.patrol_anchor, anchor);
            assert_eq!(enemy.hate, 0.0);
            assert_eq!(enemy.mood, Mood::Roam);
        }
    }
    #[test]
    fn drifting_patrol_centres_take_the_walkable_detour() {
        let mut map = open_room(65);
        for z in 1..60 {
            map.set_wall(32, z, true);
        }
        let mut player = Player::new(&map);
        player.camera.position.x = 101.0;
        player.camera.position.z = 21.0;
        let mut enemy = Entity::at_cell(&map, [10, 10], 42);
        enemy.patrol_anchor = Some([10, 10]);
        enemy.patrol_drift = 32.0;
        enemy.patrol_age = 1800.0;
        let before = enemy.position;
        enemy.tick(&map, &player, 1800.0, 0.05, 0.0, true);
        let anchor = enemy.patrol_anchor.unwrap();
        assert!(anchor[0] < 32);
        assert_eq!(path(&map, [10, 10], anchor).len(), 16);
        assert_ne!(enemy.target, [50, 10]);
        assert_eq!(enemy.mood, Mood::Roam);
        assert_eq!(enemy.hate, 0.0);
        assert!((enemy.position[0] - before[0]).hypot(enemy.position[1] - before[1]) < 0.058);
    }
    #[test]
    fn chase_and_attack_reach_six_point_five_without_teleporting() {
        let m = open_room(65);
        let mut player = Player::new(&m);
        player.camera.position = Vec3::new(41.0, m.eye_height, 49.0);
        let mut enemy = locked(&m, [20, 20], &player);
        let mut saw_attack = false;
        let mut peak_speed = 0.0f32;
        for frame in 0..2400 {
            if frame < 1200 {
                player.move_by(&m, crate::player::RUN_SPEED / 60.0, 0.0);
            }
            let before = enemy.position;
            enemy.tick(
                &m,
                &player,
                10.0 + frame as f32 / 60.0,
                1.0 / 60.0,
                0.0,
                true,
            );
            let speed = (before[0] - enemy.position[0]).hypot(before[1] - enemy.position[1]) * 60.0;
            assert!(speed <= CHASE_SPEED + 0.001, "speed {speed}");
            peak_speed = peak_speed.max(speed);
            saw_attack |= enemy.attack_left > 0.0;
        }
        assert!(saw_attack);
        assert!(peak_speed > 6.4, "peak speed {peak_speed}");
        assert!((CHASE_SPEED - 6.5).abs() < 0.0001);
        assert!((CHASE_SPEED / crate::player::RUN_SPEED - 1.3).abs() < 0.0001);
    }
    #[test]
    fn a_substep_thin_corner_cannot_be_reported_as_a_clear_shortcut() {
        let mut m = open_room(17);
        m.set_wall(8, 8, true);
        let from = [15.45, 16.10];
        let to = [16.10, 15.45];
        assert!(body_fits(&m, from, 0.0) && body_fits(&m, to, 0.0));
        assert!(!body_fits(&m, [15.775, 15.775], 0.0));
        assert!(
            !clear_segment(&m, from, to),
            "shortcut crosses an expanded corner"
        );
    }
    #[test]
    fn entity_radius_is_not_added_on_top_of_player_radius() {
        let mut m = open_room(17);
        m.set_wall(8, 8, true);
        assert!(body_fits(&m, [15.70, 17.0], 0.0));
        assert!(!body_fits(&m, [15.80, 17.0], 0.0));
    }
    #[test]
    fn committed_strikes_cover_flanks_but_not_the_back_or_walls() {
        let mut map = open_room(17);
        let mut player = Player::new(&map);
        for degrees in [-110.0_f32, -70.0, 70.0, 110.0] {
            let mut enemy = locked(&map, [8, 8], &player);
            enemy.yaw = 0.0;
            enemy.attack_left = ATTACK_DURATION - STRIKE_TIME + 0.005;
            let angle = degrees.to_radians();
            player.camera.position = Vec3::new(
                17.0 + angle.sin() * 0.65,
                map.eye_height,
                17.0 + angle.cos() * 0.65,
            );
            assert_eq!(
                enemy.tick(&map, &player, 10.0, 0.01, 0.0, true),
                degrees.abs() < 75.0,
                "strike at {degrees} degrees"
            );
        }
        for wall in [false, true] {
            map.set_wall(8, 8, wall);
            let mut enemy = locked(&map, [7, 8], &player);
            enemy.position = [15.7, 16.65];
            enemy.yaw = core::f32::consts::PI * 0.75;
            enemy.attack_left = ATTACK_DURATION - STRIKE_TIME + 0.005;
            player.camera.position = Vec3::new(16.65, map.eye_height, 15.7);
            assert_eq!(enemy.tick(&map, &player, 10.0, 0.01, 0.0, true), !wall);
        }
    }
    #[test]
    fn attack_windup_and_repeat_period_match_the_new_timing() {
        let map = Map::generated(7, 1).unwrap();
        let player = Player::new(&map);
        let mut enemy = Entity::at_cell(&map, [1, 1], 1);
        enemy.position = [3.0, 3.9];
        enemy.yaw = core::f32::consts::PI;
        enemy.tick(&map, &player, 10.0, 0.01, 0.0, true);
        let mut hits = Vec::new();
        for i in 1..=230 {
            if enemy.tick(
                &map,
                &player,
                10.0 + i as u32 as f32 * 0.01,
                0.01,
                0.0,
                true,
            ) {
                hits.push(i as u32 as f32 * 0.01);
            }
        }
        assert_eq!(hits.len(), 3);
        assert!((0.09..=0.12).contains(&hits[0]));
        for pair in hits.windows(2) {
            assert!((0.98..=1.02).contains(&(pair[1] - pair[0])));
        }
    }
}
