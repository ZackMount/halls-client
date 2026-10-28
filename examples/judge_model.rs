//! Organizer-only, version-pinned replay model. No part is linked into halls.exe.
use halls::{entity, game::Game, math::Vec3, player::Player, world::Map};
use std::io::{self, Read, Write};

fn snapshot(game: &Game, player: &Player) -> [u32; 84] {
    let mut row = [0; 84];
    let p = player.camera;
    let c = game.visual_camera(player);
    for (at, value) in [
        p.position.x,
        p.position.y,
        p.position.z,
        p.yaw,
        p.pitch,
        player.distance,
        c.position.x,
        c.position.y,
        c.position.z,
        c.yaw,
        c.pitch,
        c.roll,
        c.fov,
        game.progress.health,
        game.knocked,
        game.shock,
        game.progress.seconds,
    ]
    .into_iter()
    .enumerate()
    {
        row[6 + at] = value.to_bits();
    }
    row[23] = player.won as u32 | ((game.dead() as u32) << 1);
    for (i, e) in game.progress.entities.iter().enumerate() {
        let k = 24 + i * 12;
        for (at, value) in [
            e.position[0],
            e.position[1],
            e.yaw,
            e.travel,
            e.move_blend,
            e.attack_left,
            e.hate,
            e.speed,
            e.last_seen,
            e.cooldown,
            e.replan,
        ]
        .into_iter()
        .enumerate()
        {
            row[k + at] = value.to_bits();
        }
        row[k + 11] = e.mood as u32;
    }
    row
}

// Condition each step on the observed public state, retaining the independently
// reconstructed path, RNG, goal, idle/sight timers, strike flag and damage immunity.
// This limits cross-platform rounding drift without accepting the next state.
fn anchor(row: &[u32], game: &mut Game, player: &mut Player) {
    let f = |n: usize| f32::from_bits(row[n]);
    player.camera.position = Vec3::new(f(6), f(7), f(8));
    player.camera.yaw = f(9);
    player.camera.pitch = f(10);
    player.distance = f(11);
    player.won = row[23] & 1 != 0;
    game.progress.health = f(19);
    game.knocked = f(20);
    game.shock = f(21);
    game.progress.seconds = f(22);
    for (i, e) in game.progress.entities.iter_mut().enumerate() {
        let k = 24 + i * 12;
        e.position = [f(k), f(k + 1)];
        e.yaw = f(k + 2);
        e.travel = f(k + 3);
        e.move_blend = f(k + 4);
        e.attack_left = f(k + 5);
        e.hate = f(k + 6);
        e.speed = f(k + 7);
        e.last_seen = f(k + 8);
        e.cooldown = f(k + 9);
        e.replan = f(k + 10);
        e.mood = [
            entity::Mood::Roam,
            entity::Mood::Investigate,
            entity::Mood::Chase,
            entity::Mood::Search,
        ][row[k + 11] as usize];
    }
}

// Each core frame has one little-endian exit coordinate pair.
fn recording_payload(count: usize, input: &[u8]) -> Result<(usize, &[u8]), &'static str> {
    if !(2..=900_000).contains(&count) {
        return Err("model frame count");
    }
    let end = 60 + count * 336;
    let exit_bytes = count * 8;
    if input.len() != end + exit_bytes {
        return Err("model length/exit trace");
    }
    let trace = &input[end..];
    for &value in trace.as_chunks::<4>().0 {
        if u32::from_le_bytes(value) >= 256 {
            return Err("model exit trace range");
        }
    }
    Ok((end, trace))
}

fn tick_exit(trace: &[u8], tick: usize) -> [usize; 2] {
    let at = tick * 8;
    [
        u32::from_le_bytes(trace[at..at + 4].try_into().unwrap()) as usize,
        u32::from_le_bytes(trace[at + 4..at + 8].try_into().unwrap()) as usize,
    ]
}

fn recorded_input_victory(
    map: &Map,
    game: &Game,
    player: &Player,
    row: &[u32],
    last: bool,
) -> bool {
    let f = |n: usize| f32::from_bits(row[n]);
    // A collision mismatch can prevent the model position from entering an
    // exit although the recorded input already ended the client tick. Do not
    // invent an extra AI/combat tick in that case. Position errors remain in
    // the prediction and are still checked independently by the judge.
    last && row[23] == 1
        && !game.dead()
        && !player.won
        && game.knocked <= 0.0
        && f(19) > 0.0
        && row[19] == game.progress.health.to_bits()
        && row[22] == game.progress.seconds.to_bits()
        && game.progress.seconds + f(0) > game.progress.seconds
        && map.in_exit(f(6), f(8))
}

fn step(map: &Map, game: &mut Game, player: &mut Player, row: &[u32], last: bool) -> bool {
    let f = |n: usize| f32::from_bits(row[n]);
    let input_victory = recorded_input_victory(map, game, player, row, last);
    game.running = row[1] & 256 != 0;
    if row[1] & 1024 != 0 {
        game.battery.toggle();
    }
    if game.knocked <= 0.0 && !game.dead() && !player.won {
        player.turn(f(4), f(5));
        player.move_by(map, f(2), f(3));
    }
    let conditioned = input_victory && !player.won;
    if conditioned {
        player.won = true;
    }
    // Use the original early-return branch, including hit-timer decay. An
    // actual post-input hit/knockback victory advances time or changes HP and
    // therefore still executes the independent attack and damage simulation.
    game.tick(map, player, f(0));
    conditioned
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if args.len() != 2 || args[0] != "--lamps" {
            return Err("model arguments".into());
        }
        let map = Map::procedural(args[1].parse::<u64>()?)?;
        io::stdout().write_all(&halls::lighting::bits(&map))?;
        return Ok(());
    }
    let mut input = Vec::new();
    io::stdin()
        .take(256 * 1024 * 1024 + 65)
        .read_to_end(&mut input)?;
    // The server selects this engine only for the authenticated HLR5 build.
    if input.len() < 64
        || &input[..4] != b"HJM2"
        || u32::from_le_bytes(input[4..8].try_into()?) != 0x0100_0001
    {
        return Err("model build/header".into());
    }
    input.drain(4..8);
    let u64_at = |n| u64::from_le_bytes(input[n..n + 8].try_into().unwrap());
    let seed = u64_at(4);
    let forms: Vec<_> = (0..5).map(|i| u64_at(12 + i * 8)).collect();
    let exit = [
        u16::from_le_bytes(input[52..54].try_into()?) as usize,
        u16::from_le_bytes(input[54..56].try_into()?) as usize,
    ];
    let count = u32::from_le_bytes(input[56..60].try_into()?) as usize;
    if exit.iter().any(|&n| n >= 256) {
        return Err("model header exit".into());
    }
    let (frame_end, exit_trace) = recording_payload(count, &input)?;
    let words: Vec<u32> = input[60..frame_end]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    // Independent hard bounds protect the model from untrusted signed input.
    for row in words.as_chunks::<84>().0 {
        let f = |i| f32::from_bits(row[i]);
        for (i, &value) in row.iter().enumerate() {
            if i == 1 || i == 23 || (i >= 24 && (i - 24) % 12 == 11) {
                continue;
            }
            if !f32::from_bits(value).is_finite() || f32::from_bits(value).abs() > 1.0e7 {
                return Err("model numeric range".into());
            }
        }
        if !(0.0..=0.1).contains(&f(0))
            || (2..6).any(|i| f(i).abs() > 1.0)
            || !(0.0..512.0).contains(&f(6))
            || !(0.0..512.0).contains(&f(8))
        {
            return Err("model input range".into());
        }
        for k in (24..84).step_by(12) {
            if !(0.0..512.0).contains(&f(k))
                || !(0.0..512.0).contains(&f(k + 1))
                || row[k + 11] > 3
                || !(0.0..=10.0).contains(&f(k + 7))
            {
                return Err("model entity range".into());
            }
        }
    }
    let mut map = Map::procedural(seed)?;
    let canonical_exit = map.exit;
    let run_seed = forms[0] ^ seed ^ 0x9e3779b97f4a7c15;
    let mut game = Game::new(&map);
    game.progress.entities = entity::spawn_with_seed(&map, run_seed);
    if game.progress.entities.len() != 5 {
        return Err("model entity count".into());
    }
    let mut player = Player::new(&map);
    let mut out = io::BufWriter::new(io::stdout().lock());
    out.write_all(b"HJO2")?;
    for n in canonical_exit {
        out.write_all(&(n as u16).to_le_bytes())?;
    }
    for e in &game.progress.entities {
        out.write_all(&e.form_seed.to_le_bytes())?;
    }
    for byte in 0..8192 {
        let bits = (0..8).fold(0u8, |bits, b| {
            let i = byte * 8 + b;
            bits | ((map.wall(i % 256, i / 256) as u8) << b)
        });
        out.write_all(&[bits])?;
    }
    // Exit policy is separate from dynamics: even a shortened test course can
    // be evaluated, but the canonical-exit rule rejects it by default.
    for (tick, row) in words.as_chunks::<84>().0.iter().enumerate() {
        map.exit = tick_exit(exit_trace, tick);
        if tick != 0 && step(&map, &mut game, &mut player, row, tick + 1 == count) {
            eprintln!("HALLS_TERMINAL_CONDITIONING recorded_exit");
        }
        for value in snapshot(&game, &player) {
            out.write_all(&value.to_le_bytes())?;
        }
        anchor(row, &mut game, &mut player);
    }
    out.write_all(&halls::lighting::bits(&map))?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> (Map, Game, Player) {
        let mut map = Map::generated(17, 42).unwrap();
        for z in 1..16 {
            for x in 1..16 {
                map.set_wall(x, z, false);
            }
        }
        map.spawn.x = 3.7;
        map.spawn.z = 5.0;
        map.spawn.yaw = core::f32::consts::FRAC_PI_2;
        map.exit = [2, 2];
        let mut game = Game::new(&map);
        game.progress.seconds = 10.0;
        let player = Player::new(&map);
        (map, game, player)
    }

    fn victory_row(game: &Game, player: &Player) -> [u32; 84] {
        let mut row = snapshot(game, player);
        row[0] = 0.05f32.to_bits();
        row[2] = 0.2f32.to_bits();
        row[6] = 4.05f32.to_bits();
        row[8] = 5.0f32.to_bits();
        row[23] = 1;
        row
    }

    #[test]
    fn exit_trace_is_required_and_bounded() {
        let count = 2;
        let mut input = vec![0; 60 + count * 336];
        assert!(recording_payload(count, &input).is_err());
        for pair in [[7u32, 9], [11, 13]] {
            for value in pair {
                input.extend_from_slice(&value.to_le_bytes());
            }
        }
        let (_, trace) = recording_payload(count, &input).unwrap();
        assert_eq!(tick_exit(trace, 0), [7, 9]);
        assert_eq!(tick_exit(trace, 1), [11, 13]);
        assert!(recording_payload(count, &input[..input.len() - 1]).is_err());
        let at = input.len() - 4;
        input[at..].copy_from_slice(&256u32.to_le_bytes());
        assert!(recording_payload(count, &input).is_err());
    }

    #[test]
    fn collision_victory_keeps_prediction_error_without_an_extra_entity_tick() {
        let (mut map, mut game, mut player) = scene();
        map.set_wall(2, 2, true);
        let row = victory_row(&game, &player);
        let before = snapshot(&game, &player);
        assert!(recorded_input_victory(&map, &game, &player, &row, true));
        step(&map, &mut game, &mut player, &row, true);
        let after = snapshot(&game, &player);
        assert!(player.won);
        assert!(!map.in_exit(player.camera.position.x, player.camera.position.z));
        assert_ne!(after[6], row[6]);
        assert_eq!(game.progress.seconds, 10.0);
        assert_eq!(&after[24..], &before[24..]);
        assert_eq!(game.progress.health, 100.0);
    }

    #[test]
    fn positive_health_alone_does_not_select_the_early_return_branch() {
        let (map, mut game, player) = scene();
        let row = victory_row(&game, &player);
        assert!(!recorded_input_victory(&map, &game, &player, &row, false));
        for (field, value) in [
            (23, 0),
            (23, 3),
            (6, 3.8f32.to_bits()),
            (19, 66.0f32.to_bits()),
            (22, 10.05f32.to_bits()),
            (0, 0.0f32.to_bits()),
            (0, 0.000000001f32.to_bits()),
        ] {
            let mut altered = row;
            altered[field] = value;
            assert!(!recorded_input_victory(
                &map, &game, &player, &altered, true
            ));
        }
        game.knocked = 0.01;
        assert!(!recorded_input_victory(&map, &game, &player, &row, true));
        game.knocked = 0.0;
        game.progress.health = 0.0;
        assert!(!recorded_input_victory(&map, &game, &player, &row, true));
    }

    #[test]
    fn observed_win_requires_entry_into_the_recorded_exit() {
        let (map, mut game, mut player) = scene();
        let mut row = victory_row(&game, &player);
        row[6] = 3.9f32.to_bits();
        assert!(!map.in_exit(3.9, 5.0));
        assert!(!recorded_input_victory(&map, &game, &player, &row, true));
        assert!(!step(&map, &mut game, &mut player, &row, true));
        assert!(!player.won);
    }

    #[test]
    fn a_normal_predicted_input_victory_needs_no_conditioning_marker() {
        let (map, mut game, mut player) = scene();
        let mut row = victory_row(&game, &player);
        row[2] = 0.4f32.to_bits();
        assert!(!step(&map, &mut game, &mut player, &row, true));
        assert!(player.won);
        assert_eq!(game.progress.seconds, 10.0);
    }

    #[test]
    fn real_knockback_victory_and_hidden_damage_still_run_the_attack_model() {
        for recorded_hp in [66.0f32, 100.0f32] {
            let (mut map, mut game, mut player) = scene();
            map.exit = [8, 8];
            player.camera.position = Vec3::new(17.0, map.eye_height, 15.7);
            player.camera.yaw = 0.0;
            let mut enemy = entity::Entity::at_cell(&map, [8, 8], 17);
            enemy.position = [17.0, 15.0];
            enemy.yaw = 0.0;
            enemy.mood = entity::Mood::Chase;
            enemy.attack_left = entity::ATTACK_DURATION - entity::STRIKE_TIME + 0.01;
            game.progress.entities = vec![enemy];
            let mut row = snapshot(&game, &player);
            row[0] = 0.02f32.to_bits();
            row[8] = 16.55f32.to_bits();
            row[19] = recorded_hp.to_bits();
            row[22] = 10.02f32.to_bits();
            row[23] = 1;
            assert!(!recorded_input_victory(&map, &game, &player, &row, true));
            step(&map, &mut game, &mut player, &row, true);
            assert!(player.won);
            assert_eq!(game.progress.health, 66.0);
            assert_eq!(game.progress.seconds, 10.02);
            assert!(game.knocked > 0.0);
        }
    }
}
