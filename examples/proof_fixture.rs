//! Developer-only short course. Uses real movement/AI ticks; never built into the player.
use halls::{entity, game::Game, player::Player, player::WALK_SPEED, proof::Recording, world::Map};
fn main() {
    let folder = std::env::args().nth(1).expect("output folder");
    std::fs::create_dir_all(&folder).unwrap();
    let mut map = Map::procedural(104729).unwrap();
    let distances = map.distances();
    let end = distances.iter().position(|&d| d == 20).unwrap();
    map.exit = [end % 256, end / 256];
    let mut game = Game::new(&map);
    game.progress.entities = entity::spawn_with_seed(&map, 17);
    let mut player = Player::new(&map);
    let start = [
        (player.camera.position.x / 2.) as usize,
        (player.camera.position.z / 2.) as usize,
    ];
    let route = entity::path(&map, start, map.exit);
    let points: Vec<_> = route
        .iter()
        .map(|&n| {
            [
                ((n as usize % 256) as f32 + 0.5) * 2.,
                ((n as usize / 256) as f32 + 0.5) * 2.,
            ]
        })
        .collect();
    let mut recording = Recording::new(&map, &game, &player);
    let mut expected: Vec<u8> = recording
        .words_for_test()
        .iter()
        .take(24 + game.progress.entities.len() * 12)
        .flat_map(|n| n.to_le_bytes())
        .collect();
    // Keep independent simulation and complete recording frames for comparison.
    let mut expected_recording: Vec<u8> = recording
        .words_for_test()
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .collect();
    assert_eq!(recording.export("premature"), Err(-1));
    let pulse = std::env::args().any(|s| s == "--pulse");
    recording.input_protocol(!pulse);
    let mut waypoint = 0;
    let dt = 1. / 30.;
    for _ in 0..30000 {
        let target = points[waypoint];
        let dx = target[0] - player.camera.position.x;
        let dz = target[1] - player.camera.position.z;
        let angle = (dx.atan2(dz) - player.camera.yaw + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let turn = angle.clamp(-1.65 * dt, 1.65 * dt);
        let forward = if angle.abs() < 0.025 {
            WALK_SPEED.mul_add(dt, 0.).min(dx.hypot(dz))
        } else {
            0.
        };
        player.turn(turn, 0.);
        player.move_by(&map, forward, 0.);
        game.tick(&map, &mut player, dt);
        let held = if forward > 0. {
            1
        } else if turn > 0. {
            32
        } else {
            16
        };
        let held = if pulse {
            (if forward > 0. { 1 } else { 0 })
                | (if turn > 0. {
                    32
                } else if turn < 0. {
                    16
                } else {
                    0
                })
        } else {
            held
        };
        recording.tick(&map, &game, &player, dt, [forward, 0., turn, 0.], held);
        expected.extend(
            recording
                .words_for_test()
                .iter()
                .take(24 + game.progress.entities.len() * 12)
                .flat_map(|n| n.to_le_bytes()),
        );
        expected_recording.extend(
            recording
                .words_for_test()
                .iter()
                .flat_map(|n| n.to_le_bytes()),
        );
        if game.dead() {
            panic!("test driver died before exit");
        }
        if player.won {
            std::fs::write(format!("{folder}/fixture.frames"), &expected).unwrap();
            std::fs::write(
                format!("{folder}/fixture.recording.frames"),
                &expected_recording,
            )
            .unwrap();
            let raw = recording.raw();
            std::fs::write(format!("{folder}/fixture.raw"), raw).unwrap();
            std::fs::write(
                format!("{folder}/fixture.raw.hlc"),
                halls::proof::compress(raw),
            )
            .unwrap();
            println!(
                "Fixture completed in {} s, {} raw bytes",
                game.progress.seconds,
                raw.len()
            );
            {
                if std::env::args().any(|s| s == "--prompt") {
                    let mut terminal = halls::terminal::Terminal::enter().unwrap();
                    halls::proof::save_prompt(&mut terminal, &mut recording, 0).unwrap();
                } else {
                    recording.export("native-client-complete").unwrap();
                    let proof = std::env::current_exe()
                        .unwrap()
                        .with_file_name("native-client-complete.hproof");
                    let saved = std::fs::read(&proof).unwrap();
                    assert!(matches!(
                        recording.export("native-client-complete"),
                        Err(17 | 80 | 183)
                    ));
                    assert_eq!(saved, std::fs::read(&proof).unwrap());
                    std::fs::write(format!("{folder}/fixture.hproof"), saved).unwrap();
                    std::fs::remove_file(proof).unwrap();
                }
            }
            return;
        }
        if dx.hypot(dz) < 0.025 {
            waypoint = (waypoint + 1).min(points.len() - 1);
        }
    }
    panic!("fixture did not finish");
}
