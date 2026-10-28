//! Test-only real-time clock recording, never linked into the player executable.
use halls::{
    game::Game,
    mesh::Mesh,
    player::Player,
    proof::Recording,
    render::Renderer,
    runtime::{Duration, Instant},
    world::Map,
};
use std::path::Path;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let folder = Path::new(&args[1]);
    std::fs::create_dir_all(folder).unwrap();
    let factor: f64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(1.0);
    let map = Map::procedural(104729).unwrap();
    let mut game = Game::new(&map);
    let mut player = Player::new(&map);
    let render_scales = args.iter().any(|s| s == "--render-scales");
    let mesh = render_scales.then(|| Mesh::from_map(&map));
    let mut renderer = Renderer::default();
    let mut record = Recording::new(&map, &game, &player);
    let mut last = Instant::now();
    let frames = args
        .iter()
        .find_map(|v| v.strip_prefix("--frames="))
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(180)
        .clamp(180, 18000);
    for i in 0..frames {
        if let Some(mesh) = &mesh {
            let scale = [1, 2, 8, 16, 32, 4][(i / 10) % 6];
            std::hint::black_box(renderer.render_scaled(mesh, player.camera, 64, 36, scale));
        }
        std::thread::sleep(Duration::from_millis(if render_scales { 2 } else { 20 }));
        if i == 75 || render_scales && i > 0 && i % 30 == 0 {
            record.pause();
            std::thread::sleep(Duration::from_millis(350));
            record.resume();
            last = Instant::now();
            std::thread::sleep(Duration::from_millis(20));
        }
        let now = Instant::now();
        let raw = ((now - last).as_nanos() as f64 * factor) as u64;
        last = now;
        record.begin_frame(raw);
        let dt = (raw as f32 / 1e9).min(0.05);
        if i == frames - 1 {
            player.camera.position.x = map.exit[0] as f32 * 2. + 1.;
            player.camera.position.z = map.exit[1] as f32 * 2. + 1.;
            player.won = true;
        }
        game.tick(&map, &mut player, dt);
        record.tick(&map, &game, &player, dt, [0.; 4], 0);
    }
    std::fs::write(folder.join("clock.raw"), record.raw()).unwrap();
    println!("clock fixture factor {factor}; raw saved");
}
