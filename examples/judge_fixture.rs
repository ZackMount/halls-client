//! Long organizer regression: real map, remote spawns, pursuit, hits and death.
use halls::{entity, game::Game, player::Player, proof::Recording, world::Map};
fn main() {
    let folder = std::env::args().nth(1).expect("output folder");
    std::fs::create_dir_all(&folder).unwrap();
    let map = Map::procedural(104729).unwrap();
    let mut player = Player::new(&map);
    let mut game = Game::new(&map);
    game.progress.entities = entity::spawn_with_seed(&map, 17);
    let mut recording = Recording::new(&map, &game, &player);
    let mut turns = 0;
    for tick in 0..18000 {
        // A reproducible input driver chases the nearest entity using the same
        // bounded input units as a keyboard. It never edits gameplay state.
        let target = game
            .progress
            .entities
            .iter()
            .min_by(|a, b| {
                let distance = |e: &entity::Entity| {
                    (e.position[0] - player.camera.position.x)
                        .hypot(e.position[1] - player.camera.position.z)
                };
                distance(a).total_cmp(&distance(b))
            })
            .unwrap();
        let start = [
            (player.camera.position.x / 2.) as usize,
            (player.camera.position.z / 2.) as usize,
        ];
        let end = [
            (target.position[0] / 2.) as usize,
            (target.position[1] / 2.) as usize,
        ];
        let path = entity::path(&map, start, end);
        let goal = path
            .front()
            .map(|&n| {
                [
                    ((n as usize % 256) as f32 + 0.5) * 2.,
                    ((n as usize / 256) as f32 + 0.5) * 2.,
                ]
            })
            .unwrap_or(target.position);
        let dx = goal[0] - player.camera.position.x;
        let dz = goal[1] - player.camera.position.z;
        let delta = (dx.atan2(dz) - player.camera.yaw + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let dt = if tick % 7 == 0 { 0.02 } else { 1. / 30. };
        let mut action = [0.; 4];
        let mut held = 0;
        game.running = true;
        if game.knocked <= 0. {
            if delta.abs() > 0.04 {
                action[2] = delta.signum() * 1.65 * dt;
                held = if delta > 0. { 32 } else { 16 };
                turns += 1;
            }
            if delta.abs() < 0.5 {
                action[0] = halls::player::RUN_SPEED * dt;
                held |= 1;
            }
            player.turn(action[2], 0.);
            player.move_by(&map, action[0], 0.);
        }
        game.tick(&map, &mut player, dt);
        recording.tick(&map, &game, &player, dt, action, held);
        if game.dead() || player.won {
            break;
        }
    }
    let raw = recording.raw();
    std::fs::write(format!("{folder}/encounter.raw"), raw).unwrap();
    println!(
        "{} s; hp {}; turn frames {}; bytes {}",
        game.progress.seconds,
        game.progress.health,
        turns,
        raw.len()
    );
}
