#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    math::{Camera, Vec3},
    world::Map,
};

pub const WALK_SPEED: f32 = 2.0;
pub const RUN_SPEED: f32 = 5.0;

pub struct Player {
    pub camera: Camera,
    pub distance: f32,
    pub won: bool,
}
impl Player {
    pub fn new(map: &Map) -> Self {
        Self {
            camera: Camera {
                position: map.spawn_position(),
                yaw: map.spawn.yaw,
                pitch: 0.0,
                roll: 0.0,
                fov: 80.0_f32.to_radians(),
            },
            distance: 0.0,
            won: false,
        }
    }
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        self.camera.yaw = (self.camera.yaw + yaw).rem_euclid(core::f32::consts::TAU);
        self.camera.pitch = (self.camera.pitch + pitch).clamp(-1.1, 1.1);
    }
    pub fn move_by(&mut self, map: &Map, forward: f32, strafe: f32) {
        if self.won {
            return;
        }
        let direction = Vec3::new(self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()) * forward
            + self.camera.right() * strafe;
        let n = (direction.length() / 0.07).ceil().max(1.0) as u32 as usize;
        let delta = direction / n as u32 as f32;
        for _ in 0..n {
            let p = self.camera.position;
            let floor = map.floor_at(p.x, p.z);
            if map.walkable(p.x + delta.x, p.z, floor) {
                self.camera.position.x += delta.x;
            }
            let floor = map.floor_at(self.camera.position.x, self.camera.position.z);
            if map.walkable(
                self.camera.position.x,
                self.camera.position.z + delta.z,
                floor,
            ) {
                self.camera.position.z += delta.z;
            }
            let moved = self.camera.position - p;
            self.distance += (moved.x * moved.x + moved.z * moved.z).sqrt();
            self.camera.position.y =
                map.floor_at(self.camera.position.x, self.camera.position.z) + map.eye_height;
        }
        self.won = map.in_exit(self.camera.position.x, self.camera.position.z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entering_any_exit_edge_wins_without_reaching_the_center() {
        let mut map = Map::generated(17, 3).unwrap();
        for z in 1..16 {
            for x in 1..16 {
                map.set_wall(x, z, false);
            }
        }
        map.exit = [8, 8];
        for (x, z, yaw) in [
            (15.9, 16.5, core::f32::consts::FRAC_PI_2),
            (18.1, 17.5, -core::f32::consts::FRAC_PI_2),
            (16.5, 15.9, 0.0),
            (17.5, 18.1, core::f32::consts::PI),
        ] {
            let mut player = Player::new(&map);
            player.camera.position = Vec3::new(x, map.eye_height, z);
            player.camera.yaw = yaw;
            player.move_by(&map, 0.0, 0.0);
            assert!(!player.won);
            player.move_by(&map, 0.15, 0.0);
            assert!(player.won);
            assert!((player.camera.position.x - 17.0).hypot(player.camera.position.z - 17.0) > 0.5);
        }
        assert!(map.in_exit(16.0, 16.0));
        assert!(map.in_exit(17.999, 17.999));
        assert!(!map.in_exit(18.0, 17.0));
        assert!(!map.in_exit(17.0, 18.0));
    }
    #[test]
    fn large_movements_cannot_tunnel_through_walls() {
        let m = Map::generated(7, 5).unwrap();
        let mut p = Player::new(&m);
        p.camera.yaw = core::f32::consts::PI;
        p.move_by(&m, 30.0, 0.0);
        assert!(p.camera.position.z >= m.tile_size + 0.22 - 1e-3);
    }
    #[test]
    fn walking_up_a_ramp_changes_camera_height() {
        let mut m = Map::generated(7, 3).unwrap();
        m.elevations.push(crate::world::Elevation {
            cell: [2, 1],
            height: 0.0,
            slope: [0.8, 0.0],
        });
        let mut p = Player::new(&m);
        p.move_by(&m, 1.8, 0.0);
        assert!(p.camera.position.y > m.eye_height + 0.15);
    }
}
