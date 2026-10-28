#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    entity::{self, Entity},
    math::{Camera, Vec3},
    mesh::{Material, Mesh},
    player::Player,
    world::Map,
};
#[derive(Clone)]
pub struct Progress {
    pub seconds: f32,
    pub health: f32,
    pub entities: Vec<Entity>,
}
pub struct Game {
    pub progress: Progress,
    pub flashlight: bool,
    pub battery: crate::battery::Battery,
    pub coins: crate::coins::Coins,
    pub message: String,
    pub running: bool,
    pub knocked: f32,
    pub shock: f32,
    pub death_time: f32,
    message_until: f32,
    last_distance: f32,
    foot_distance: f32,
    iframe: f32,
    last_position: Vec3,
}
impl Game {
    pub fn new(map: &Map) -> Self {
        Self {
            progress: Progress {
                seconds: 0.0,
                health: 100.0,
                entities: entity::spawn(map),
            },
            flashlight: true,
            battery: crate::battery::Battery::default(),
            coins: crate::coins::Coins::new(map),
            message: String::new(),
            running: false,
            knocked: 0.0,
            shock: 0.0,
            death_time: 0.0,
            message_until: 0.0,
            last_distance: 0.0,
            foot_distance: 0.0,
            iframe: 0.0,
            last_position: map.spawn_position(),
        }
    }
    pub fn dead(&self) -> bool {
        self.progress.health <= 0.0
    }
    pub fn say(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.message_until = self.progress.seconds + 3.0;
    }
    pub fn tick(&mut self, map: &Map, player: &mut Player, dt: f32) {
        self.knocked = (self.knocked - dt).max(0.0);
        self.shock = (self.shock - dt).max(0.0);
        self.iframe = (self.iframe - dt).max(0.0);
        self.last_position = player.camera.position;
        if self.dead() {
            self.coins.tick(map, player.camera.position, dt, false);
            self.death_time += dt;
            player.won = false;
            return;
        }
        if player.won {
            self.coins.tick(map, player.camera.position, dt, false);
            return;
        }
        self.battery.tick(&mut self.flashlight, dt);
        self.progress.seconds += dt;
        if self.progress.seconds > self.message_until {
            self.message.clear();
        }
        let moved = (player.distance - self.last_distance).max(0.0);
        self.last_distance = player.distance;
        self.foot_distance += moved;
        let stride = if self.running { 0.80 } else { 0.65 };
        let mut noise = 0.0;
        if moved > 0.001 && self.foot_distance > stride {
            self.foot_distance -= stride;
            noise = if self.running {
                entity::HEARING_RUN
            } else {
                entity::HEARING_WALK
            };
        }
        let mut attacked = None;
        for enemy in &mut self.progress.entities {
            let encounter = enemy.tick(
                map,
                player,
                self.progress.seconds,
                dt,
                noise,
                self.flashlight,
            );
            if encounter && self.iframe <= 0.0 {
                attacked = Some(enemy.position);
            }
        }
        if let Some(at) = attacked {
            self.progress.health = (self.progress.health - 34.0).max(0.0);
            self.knocked = 0.18;
            self.shock = 0.30;
            self.iframe = 2.2;
            let mut away = Vec3::new(
                player.camera.position.x - at[0],
                0.0,
                player.camera.position.z - at[1],
            )
            .normalized()
                * 0.85;
            if away.length() < 0.01 {
                away = Vec3::new(player.camera.yaw.sin(), 0.0, player.camera.yaw.cos()) * -0.85;
            }
            let forward = Vec3::new(player.camera.yaw.sin(), 0.0, player.camera.yaw.cos());
            player.move_by(map, away.dot(forward), away.dot(player.camera.right()));
            self.last_distance = player.distance;
            if self.dead() {
                player.won = false;
                self.running = false;
                self.say("YOU DIED");
            } else {
                self.say("HIT — MOVE!");
            }
        }
        // The proof records this final position, including any knockback.
        // Collection must use the same state as the independent judge.
        if self
            .coins
            .tick(map, player.camera.position, dt, !self.dead())
            && self.knocked <= 0.0
        {
            self.say("COIN COLLECTED");
        }
        self.last_position = player.camera.position;
    }
    pub fn visual_camera(&self, player: &Player) -> Camera {
        let mut camera = player.camera;
        if self.dead() {
            camera.position.y -= 1.12 * (self.death_time / 0.9).min(1.0);
            camera.pitch = -0.18;
            camera.roll = -0.48 * (self.death_time / 0.9).min(1.0);
        } else if self.knocked > 0.0 {
            camera.position.y -= 0.14 * (self.knocked / 0.18);
            camera.pitch -= 0.08 * (self.knocked / 0.18);
            camera.roll = (self.progress.seconds * 16.0).sin() * 0.025 * (self.knocked / 0.18);
        }
        if self.shock > 0.0 {
            camera.yaw += (self.progress.seconds * 54.0).sin() * 0.015 * self.shock;
        }
        camera
    }
    pub fn add_geometry(&self, map: &Map, mesh: &mut Mesh) {
        self.coins.draw(map, mesh, self.last_position);
        let [x0, z0, x1, z1] = map.exit_bounds();
        let x = (x0 + x1) * 0.5;
        let z = (z0 + z1) * 0.5;
        let dx = x - self.last_position.x;
        let dz = z - self.last_position.z;
        if dx * dx + dz * dz < 100.0 * 100.0 {
            let first = mesh.triangles.len();
            // Reuse the entity's VM, orbit/lifetime formulas and crossed quads.
            // Negative scale draws only that existing cloud; no second emitter.
            entity::draw_wire_pose(
                mesh,
                Vec3::new(x, map.floor_at(x, z), z),
                0.0,
                self.progress.seconds,
                0,
                -map.ceiling / 3.5,
                entity::Motion {
                    rage: 1.0,
                    ..Default::default()
                },
            );
            for triangle in &mut mesh.triangles[first..] {
                triangle.material = Material::Green;
            }
        }
        for enemy in &self.progress.entities {
            let dx = enemy.position[0] - self.last_position.x;
            let dz = enemy.position[1] - self.last_position.z;
            if dx * dx + dz * dz < 100.0 * 100.0 {
                enemy.draw(map, mesh, self.progress.seconds);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn green_floor_covers_exactly_the_exit_trigger() {
        let map = Map::generated(17, 4).unwrap();
        let mesh = Mesh::from_map(&map);
        let green: Vec<_> = mesh
            .triangles
            .iter()
            .filter(|t| t.material == Material::Green)
            .collect();
        assert_eq!(green.len(), 2);
        let [x0, z0, x1, z1] = map.exit_bounds();
        for x in [x0, x1] {
            for z in [z0, z1] {
                assert!(
                    green
                        .iter()
                        .flat_map(|t| &t.vertices)
                        .any(|v| v.p.x == x && v.p.z == z && v.p.y == 0.0)
                );
            }
        }
    }
    #[test]
    fn exit_requires_no_switches() {
        let mut map = Map::generated(7, 1).unwrap();
        map.breakers = vec![[1, 1]];
        let mut p = Player::new(&map);
        p.won = true;
        let mut g = Game::new(&map);
        g.tick(&map, &mut p, 0.02);
        assert!(p.won);
    }
    #[test]
    fn repeated_attacks_cause_knockdown_and_death() {
        let mut map = Map::generated(7, 1).unwrap();
        map.entities = vec![[1, 1]];
        let mut p = Player::new(&map);
        let mut g = Game::new(&map);
        g.progress.seconds = 10.0;
        g.progress.entities = vec![Entity::at_cell(&map, [1, 1], 1)];
        for _ in 0..3 {
            let e = &mut g.progress.entities[0];
            e.position = [p.camera.position.x, p.camera.position.z + 0.7];
            e.yaw = core::f32::consts::PI;
            e.cooldown = 0.0;
            e.attack_left = entity::ATTACK_DURATION - entity::STRIKE_TIME + 0.01;
            e.strike_done = false;
            e.mood = crate::entity::Mood::Chase;
            g.iframe = 0.0;
            g.tick(&map, &mut p, 0.02);
        }
        assert!(g.dead());
        assert!(g.knocked > 0.0);
        assert!(!p.won);
    }
    #[test]
    fn coin_collection_uses_final_knockback_position_and_requires_survival() {
        for (start_z, enemy_delta, health, expected) in [
            (16.1, 0.7, 100.0, 0),
            (15.7, -0.7, 100.0, 1),
            (15.7, -0.7, 34.0, 0),
        ] {
            let mut map = Map::generated(17, 1).unwrap();
            for z in 1..16 {
                for x in 1..16 {
                    map.set_wall(x, z, false);
                }
            }
            map.spawn.x = 17.0;
            map.spawn.z = start_z;
            map.exit = [15, 15];
            let mut player = Player::new(&map);
            let mut game = Game::new(&map);
            game.coins.cells = vec![[8, 8]];
            game.progress.seconds = 10.0;
            game.progress.health = health;
            let mut enemy = Entity::at_cell(&map, [8, 8], 17);
            enemy.position = [17.0, start_z + enemy_delta];
            enemy.yaw = if enemy_delta > 0.0 {
                core::f32::consts::PI
            } else {
                0.0
            };
            enemy.mood = entity::Mood::Chase;
            enemy.attack_left = entity::ATTACK_DURATION - entity::STRIKE_TIME + 0.01;
            game.progress.entities = vec![enemy];
            game.tick(&map, &mut player, 0.02);
            assert_eq!(game.progress.health, (health - 34.0).max(0.0));
            assert_eq!(game.coins.collected(), expected);
            assert_eq!(
                (player.camera.position.z / map.tile_size) as usize == 8,
                enemy_delta < 0.0
            );
        }
    }
    #[test]
    fn sprinting_after_one_hit_gets_outside_the_next_attack() {
        let mut map = Map::generated(17, 1).unwrap();
        for z in 1..16 {
            for x in 1..16 {
                map.set_wall(x, z, false);
            }
        }
        map.spawn.x = 17.0;
        map.spawn.z = 17.0;
        map.spawn.yaw = core::f32::consts::PI;
        map.exit = [15, 15];
        let mut p = Player::new(&map);
        let mut g = Game::new(&map);
        g.progress.seconds = 10.0;
        let mut enemy = Entity::at_cell(&map, [8, 8], 1);
        enemy.position[1] += 0.8;
        enemy.yaw = core::f32::consts::PI;
        g.progress.entities = vec![enemy];
        for _ in 0..40 {
            g.tick(&map, &mut p, 0.02);
            if g.progress.health < 100.0 {
                break;
            }
        }
        assert_eq!(g.progress.health, 66.0);
        let hit_at = g.progress.seconds;
        let z = p.camera.position.z;
        let mut first_move = None;
        for _ in 0..125 {
            g.running = true;
            if g.knocked <= 0.0 {
                p.move_by(&map, 4.6 * 0.02, 0.0);
                if first_move.is_none() {
                    first_move = Some(g.progress.seconds - hit_at);
                }
            }
            g.tick(&map, &mut p, 0.02);
        }
        assert!(first_move.unwrap() < 0.25);
        assert!(p.camera.position.z < z - 8.0);
        assert_eq!(g.progress.health, 66.0);
    }
    #[test]
    fn running_does_not_exhaust_and_standing_does_not_emit_steps() {
        let mut map = Map::generated(65, 1).unwrap();
        for z in 1..64 {
            for x in 1..64 {
                map.set_wall(x, z, false);
            }
        }
        map.spawn.x = 65.0;
        map.spawn.z = 65.0;
        let mut player = Player::new(&map);
        let mut game = Game::new(&map);
        game.progress.seconds = 10.0;
        game.running = true;
        let mut enemy = Entity::at_cell(&map, [32, 45], 1);
        enemy.idle = 100.0;
        enemy.replan = 100.0;
        game.progress.entities = vec![enemy];
        game.foot_distance = 0.9;
        for _ in 0..100 {
            game.tick(&map, &mut player, 0.05);
        }
        assert_eq!(game.progress.entities[0].mood, entity::Mood::Roam);
        player.move_by(&map, 0.1, 0.0);
        game.tick(&map, &mut player, 0.05);
        assert_eq!(game.progress.entities[0].mood, entity::Mood::Investigate);
        game.progress.entities.clear();
        let start = player.distance;
        for _ in 0..240 {
            player.move_by(&map, 4.6 * 0.05, 0.0);
            game.tick(&map, &mut player, 0.05);
            assert!(game.running);
        }
        assert!(player.distance > start + 55.0);
    }
}
