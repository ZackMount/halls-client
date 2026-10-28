use halls::error::{Context, Result, bail};
use halls::{
    app,
    game::Game,
    math::{Camera, Vec3},
    mesh::Mesh,
    player::Player,
    render::Renderer,
    world::Map,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
fn option(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}
fn parse<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> Result<T> {
    match option(args, flag) {
        Some(s) => s
            .parse()
            .map_err(|_| halls::error::Error::new(format!("Invalid value for {flag}"))),
        None => Ok(default),
    }
}
fn dimensions(args: &[String], default: &str) -> Result<(usize, usize)> {
    let size = option(args, "--size").unwrap_or(default.into());
    let (w, h) = size.split_once('x').context("Size must be WIDTHxHEIGHT")?;
    let (w, h) = (w.parse::<usize>()?, h.parse::<usize>()?);
    if !(16..=1920).contains(&w) || !(16..=1080).contains(&h) {
        bail!("Size must be between 16x16 and 1920x1080");
    }
    Ok((w, h))
}
fn load(args: &[String]) -> Result<Map> {
    match args
        .first()
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
    {
        Some(p) => Ok(Map::load(&p)?),
        None => Ok(Map::procedural(42)?),
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let rest = args.get(1..).unwrap_or(&[]);
    match command {
        "help" | "--help" => println!(
            "HALLS developer tools: generate / verify / render / bench / sound-demo. No interactive editor."
        ),
        "sound-demo" => {
            let seconds = parse(rest, "--seconds", 20u32)?;
            println!("Procedural audio demo: {seconds}s; no audio files are read or written.");
            if !halls::audio::demo(seconds) {
                bail!("No active playback device");
            }
            println!("Playback completed.");
        }
        "panic-check" => {
            let _terminal = halls::terminal::Terminal::enter()?;
            panic!("Deliberate terminal restoration check");
        }
        "play" => {
            let map = load(rest)?;
            let mut game = Game::new(&map);
            let mut player = Player::new(&map);
            if rest.iter().any(|a| a == "--dead") {
                game.progress.health = 0.0;
            }
            if rest.iter().any(|a| a == "--won") {
                player.won = true;
            }
            let outcome = app::preview(map.clone(), game, player)?;
            if outcome.return_menu {
                let mut scale = 1;
                let mut next_map = map;
                while halls::menu::start(&next_map, &mut scale)? != halls::menu::Choice::Quit {
                    next_map = Map::procedural(halls::world::fresh_seed())?;
                    if !app::run(next_map.clone(), &mut scale)?.return_menu {
                        break;
                    }
                }
            }
        }
        "generate" => {
            let seed = parse(rest, "--seed", 42u64)?;
            let map = Map::procedural(seed)?;
            let out = option(rest, "--out").context("Specify --out map.json")?;
            map.save(Path::new(&out))?;
            println!(
                "Saved {out}: {}x{}, {} entity spawn candidates",
                map.width(),
                map.height(),
                map.entities.len()
            );
        }
        "verify" => {
            let map = load(rest)?;
            println!(
                "Valid: {}x{}, {} route steps, {} entity spawn candidates, {} active entities, {} triangles",
                map.width(),
                map.height(),
                map.shortest_route().context("No route")?.len(),
                map.entities.len(),
                halls::entity::spawn_with_seed(&map, 0).len(),
                Mesh::from_map(&map).triangles.len()
            );
        }
        "render" | "bench" => {
            let map = load(rest)?;
            let mut mesh = Mesh::from_map(&map);
            let mut game = Game::new(&map);
            game.progress.seconds = parse(rest, "--time", 0.0)?;
            game.add_geometry(&map, &mut mesh);
            let (w, h) = dimensions(
                rest,
                if command == "bench" {
                    "160x90"
                } else {
                    "320x180"
                },
            )?;
            let mut camera = Camera {
                position: map.spawn_position(),
                yaw: map.spawn.yaw,
                pitch: 0.0,
                roll: 0.0,
                fov: parse(rest, "--fov", 80f32)?.to_radians(),
            };
            if let Some(pos) = option(rest, "--pos") {
                let xyz: Vec<_> = pos
                    .split(',')
                    .map(str::parse::<f32>)
                    .collect::<std::result::Result<_, _>>()?;
                if xyz.len() != 3 || xyz.iter().any(|v| !v.is_finite()) {
                    bail!("Position must be finite x,y,z");
                }
                camera.position = Vec3::new(xyz[0], xyz[1], xyz[2]);
            }
            camera.yaw = parse(rest, "--yaw", camera.yaw.to_degrees())?.to_radians();
            camera.pitch = parse(rest, "--pitch", 0f32)?.to_radians();
            if !camera.yaw.is_finite()
                || !camera.pitch.is_finite()
                || !camera.fov.is_finite()
                || !(30.0..=120.0).contains(&camera.fov.to_degrees())
            {
                bail!("Invalid camera angles");
            }
            let aa = parse(
                rest,
                "--aa",
                if command == "bench" { 1usize } else { 2usize },
            )?;
            if !(1..=2).contains(&aa) {
                bail!("AA must be 1 or 2");
            }
            let mut renderer = Renderer {
                pixel_aspect: parse(rest, "--pixel-aspect", 1.0)?,
                retro: !rest.iter().any(|a| a == "--clean"),
                flashlight: !rest.iter().any(|a| a == "--no-lamp"),
                ..Default::default()
            };
            if !renderer.pixel_aspect.is_finite() || !(0.5..=2.0).contains(&renderer.pixel_aspect) {
                bail!("Pixel aspect must be between 0.5 and 2.0");
            }
            let frames = if command == "bench" {
                parse(rest, "--frames", 120usize)?
            } else {
                1
            };
            if frames == 0 || frames > 10000 {
                bail!("Frames must be between 1 and 10000");
            }
            let start = Instant::now();
            let mut last = None;
            for i in 0..frames {
                let mut c = camera;
                if command == "bench" {
                    c.yaw += i as f32 * 0.015;
                }
                let frame = renderer.render(&mesh, c, w * aa, h * aa);
                last = Some(if aa > 1 { frame.downsample(aa) } else { frame });
            }
            let elapsed = start.elapsed().as_secs_f64();
            if command == "render" {
                let out = option(rest, "--out").context("Specify --out view.ppm")?;
                last.unwrap().ppm(Path::new(&out))?;
                println!(
                    "Rendered {out}: {w}x{h}, {} visible / {} triangles, {:.2} ms",
                    renderer.stats.visible,
                    renderer.stats.submitted,
                    elapsed * 1000.0
                );
            } else {
                println!(
                    "CPU rasterizer: {w}x{h}, {frames} frames, {:.2} ms/frame ({:.1} fps). Terminal I/O is excluded.",
                    elapsed * 1000.0 / frames as f64,
                    frames as f64 / elapsed
                );
            }
        }
        _ => bail!("Unknown developer command"),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("halls-tools: {error:#}");
        std::process::exit(1);
    }
}
