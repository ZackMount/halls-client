use crate::error::Result;
use crate::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
#[allow(unused_imports)]
use crate::prelude::*;
use crate::runtime::{Duration, Instant};
use crate::{
    audio::Audio,
    game::Game,
    menu::{self, Pause},
    mesh::Mesh,
    player::{Player, RUN_SPEED, WALK_SPEED},
    render::Renderer,
    terminal::Terminal,
    world::{Map, fresh_seed},
};

pub struct Outcome {
    pub return_menu: bool,
}
struct Input {
    held: u8,
    fast: u8,
    actions: u8,
    pulse: [f32; 4],
    run_override: bool,
    shift_until: Instant,
}
impl Input {
    fn new() -> Self {
        Self {
            held: 0,
            fast: 0,
            actions: 0,
            pulse: [0.0; 4],
            run_override: false,
            shift_until: Instant::now(),
        }
    }
    fn clear(&mut self) {
        self.stop_motion();
        self.actions = 0;
    }
    fn stop_motion(&mut self) {
        self.held = 0;
        self.fast = 0;
        self.pulse = [0.0; 4];
        self.shift_until = Instant::now();
    }
    fn movement(&mut self, key: KeyEvent, key_releases: bool) {
        let code = normalize(key.code);
        let bit = motion_bit(code);
        if key.kind == KeyEventKind::Release {
            self.held &= !bit;
            self.fast &= !bit;
            return;
        }
        if key_releases {
            self.held |= bit;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                self.fast |= bit;
            } else {
                self.fast &= !bit;
            }
            return;
        }
        // Classic terminals lack key-up events. Each press/repeat creates a finite
        // displacement, integrated smoothly over time; keys can never get stuck.
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            self.shift_until = Instant::now() + Duration::from_millis(180);
        }
        let step = if self.run_override || key.modifiers.contains(KeyModifiers::SHIFT) {
            0.32
        } else {
            0.18
        };
        match code {
            KeyCode::Char('w') => self.pulse[0] += step,
            KeyCode::Char('s') => self.pulse[0] -= step,
            KeyCode::Char('d') => self.pulse[1] += step,
            KeyCode::Char('a') => self.pulse[1] -= step,
            KeyCode::Left => self.pulse[2] -= 0.10,
            KeyCode::Right => self.pulse[2] += 0.10,
            KeyCode::Up => self.pulse[3] += 0.08,
            KeyCode::Down => self.pulse[3] -= 0.08,
            _ => {}
        }
        for p in &mut self.pulse {
            *p = p.clamp(-2.0, 2.0);
        }
    }
    fn action(&mut self, key: KeyEvent, key_releases: bool) -> bool {
        let bit = match normalize(key.code) {
            KeyCode::Char('f') => 1,
            KeyCode::Char('z') => 2,
            KeyCode::Char('m') => 4,
            _ => return key.kind != KeyEventKind::Release,
        };
        if key.kind == KeyEventKind::Release {
            self.actions &= !bit;
            return false;
        }
        if key.kind == KeyEventKind::Repeat {
            return false;
        }
        if key_releases {
            let fresh = self.actions & bit == 0;
            self.actions |= bit;
            return fresh;
        }
        // Old Unix terminals have no release events. Preserve discrete taps there.
        true
    }

    fn update(&mut self, player: &mut Player, map: &Map, dt: f32, key_releases: bool) -> [f32; 4] {
        let mut action = [0.0; 4];
        if key_releases {
            let active = |code| {
                if self.held & motion_bit(code) != 0 {
                    1.0
                } else {
                    0.0
                }
            };
            let speed = if !self.run_override {
                WALK_SPEED
            } else {
                RUN_SPEED
            };
            action[0] = (active(KeyCode::Char('w')) - active(KeyCode::Char('s'))) * speed * dt;
            action[1] = (active(KeyCode::Char('d')) - active(KeyCode::Char('a'))) * speed * dt;
            action[2] = (active(KeyCode::Right) - active(KeyCode::Left)) * 1.65 * dt;
            action[3] = (active(KeyCode::Up) - active(KeyCode::Down)) * 1.1 * dt;
        } else {
            for (i, value) in action.iter_mut().enumerate() {
                let limit = if i < 2 {
                    if self.run_override {
                        RUN_SPEED * dt
                    } else {
                        WALK_SPEED * dt
                    }
                } else if i == 2 {
                    1.65 * dt
                } else {
                    1.1 * dt
                };
                *value = self.pulse[i].clamp(-limit, limit);
                self.pulse[i] -= *value;
            }
        }
        let limit = if self.run_override {
            RUN_SPEED
        } else {
            WALK_SPEED
        } * dt;
        let length = (action[0] * action[0] + action[1] * action[1]).sqrt();
        if length > limit && length > 0. {
            action[0] *= limit / length;
            action[1] *= limit / length;
        }
        player.turn(action[2], action[3]);
        player.move_by(map, action[0], action[1]);
        action
    }
}
fn normalize(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    }
}
fn is_motion(code: KeyCode) -> bool {
    matches!(
        normalize(code),
        KeyCode::Char('w' | 'a' | 's' | 'd')
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
    )
}

fn motion_bit(code: KeyCode) -> u8 {
    match code {
        KeyCode::Char('w') => 1,
        KeyCode::Char('s') => 2,
        KeyCode::Char('a') => 4,
        KeyCode::Char('d') => 8,
        KeyCode::Left => 16,
        KeyCode::Right => 32,
        KeyCode::Up => 64,
        KeyCode::Down => 128,
        _ => 0,
    }
}
pub fn run(map: Map, scale: &mut usize) -> Result<Outcome> {
    let game = Game::new(&map);
    let player = Player::new(&map);
    run_scene(map, game, player, scale)
}

#[cfg(feature = "tools")]
pub fn preview(map: Map, game: Game, player: Player) -> Result<Outcome> {
    run_scene(map, game, player, &mut 1)
}

fn run_scene(
    mut map: Map,
    mut game: Game,
    mut player: Player,
    scale: &mut usize,
) -> Result<Outcome> {
    let mut terminal = Terminal::enter()?;
    let key_releases = terminal.key_releases();
    let mut mesh = Mesh::from_map(&map);
    let mut renderer = Renderer {
        flashlight: game.flashlight,
        ..Default::default()
    };
    terminal.invalidate()?;
    #[cfg(feature = "tools")]
    let muted = std::env::var("HALLS_MUTE").is_ok_and(|v| v == "1");
    #[cfg(not(feature = "tools"))]
    let muted = false;
    let mut audio = Audio::new(muted);
    audio.reset(&game, &player);
    let mut focused = true;
    let mut input = Input::new();
    let mut last = Instant::now();
    let mut needs_redraw = true;

    let mut recording = crate::proof::Recording::new(&map, &game, &player);
    let mut run_toggle = false;
    'frames: loop {
        let started = Instant::now();
        let elapsed = started - last;
        let dt = elapsed.as_secs_f32().min(0.05);
        recording.begin_frame(elapsed.as_nanos() as u64);
        last = started;
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::FocusLost => {
                    input.clear();
                    focused = false;
                    audio.pause(true);
                }
                Event::FocusGained => {
                    focused = true;
                    last = Instant::now();
                }
                Event::Resize(_, _) => {
                    terminal.invalidate()?;
                    needs_redraw = true;
                }
                Event::Key(mut key) => {
                    key.code = normalize(key.code);
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        return Ok(Outcome { return_menu: false });
                    }
                    if key.kind == KeyEventKind::Release {
                        input.movement(key, key_releases);
                        input.action(key, key_releases);
                        continue;
                    }
                    if matches!(key.code, KeyCode::Char('m' | '[' | ']')) {
                        if input.action(key, key_releases) {
                            match key.code {
                                KeyCode::Char('m') => audio.toggle(),
                                KeyCode::Char('[') => audio.volume(-0.1),
                                _ => audio.volume(0.1),
                            }
                        }
                        game.say(audio.status());
                        continue;
                    }
                    if game.dead()
                        && !matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('r'))
                    {
                        continue;
                    }
                    if player.won {
                        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                            return Ok(Outcome { return_menu: true });
                        }
                        continue;
                    }
                    if !input.action(key, key_releases) {
                        continue;
                    }
                    match key.code {
                        KeyCode::Esc => {
                            input.clear();
                            let (w, h) = terminal.size()?;
                            let frame = renderer.render_scaled(
                                &mesh,
                                game.visual_camera(&player),
                                w.max(32),
                                h.saturating_sub(1).max(11) * 2,
                                *scale,
                            );
                            audio.pause(true);
                            recording.pause();
                            match menu::pause(&mut terminal, frame, scale)? {
                                Pause::Resume => {}
                                action => {
                                    return Ok(Outcome {
                                        return_menu: action == Pause::Menu,
                                    });
                                }
                            }
                            audio.pause(false);
                            last = Instant::now();
                            needs_redraw = true;
                            recording.resume();
                            continue 'frames;
                        }
                        KeyCode::Char('f') => {
                            if game.battery.toggle() {
                                recording.flashlight_input();
                            }
                        }
                        KeyCode::Char('z') => run_toggle = !run_toggle,
                        KeyCode::Char('r') | KeyCode::Enter if game.dead() => {
                            audio.pause(true);
                            map = Map::procedural(fresh_seed())?;
                            mesh = Mesh::from_map(&map);
                            game = Game::new(&map);
                            player = Player::new(&map);
                            recording = crate::proof::Recording::new(&map, &game, &player);
                            audio.reset(&game, &player);

                            run_toggle = false;
                            input.clear();
                            renderer.flashlight = game.flashlight;
                            terminal.invalidate()?;
                            audio.pause(false);
                            last = Instant::now();
                            continue 'frames;
                        }
                        code if is_motion(code) && !player.won => input.movement(key, key_releases),
                        _ => {}
                    }
                }
            }
        }
        let (w, h) = terminal.size()?;
        audio.pause(!focused || w < 32 || h < 12);
        if !focused || w < 32 || h < 12 {
            recording.pause();
        } else if recording.resume() {
            last = Instant::now();
            continue 'frames;
        }
        if w < 32 || h < 12 {
            if needs_redraw {
                terminal.text_screen(
                    "Terminal too small.\r\nResize to at least 32x12.\r\nEsc pauses.\r\n",
                )?;
                needs_redraw = false;
            }
        } else {
            let ticking = focused && !game.dead() && !player.won;
            let mut action = [0.; 4];
            game.running = run_toggle || input.fast != 0 || Instant::now() < input.shift_until;
            input.run_override = game.running;
            if game.dead() || player.won {
                input.stop_motion();
            } else if focused && game.knocked <= 0.0 {
                action = input.update(&mut player, &map, dt, key_releases);
            }
            if focused {
                game.tick(&map, &mut player, dt);
            }
            if ticking {
                recording.input_protocol(key_releases);
                let held = if key_releases {
                    input.held
                } else {
                    (if action[0] > 0. {
                        1
                    } else if action[0] < 0. {
                        2
                    } else {
                        0
                    }) | (if action[1] > 0. {
                        8
                    } else if action[1] < 0. {
                        4
                    } else {
                        0
                    }) | (if action[2] > 0. {
                        32
                    } else if action[2] < 0. {
                        16
                    } else {
                        0
                    }) | (if action[3] > 0. {
                        64
                    } else if action[3] < 0. {
                        128
                    } else {
                        0
                    })
                };
                recording.tick(&map, &game, &player, dt, action, held);
            }
            audio.sync(&map, &game, &player);

            renderer.flashlight = game.flashlight;
            let base = mesh.triangles.len();
            game.add_geometry(&map, &mut mesh);
            let mut frame = renderer.render_scaled(
                &mesh,
                game.visual_camera(&player),
                w,
                h.saturating_sub(1) * 2,
                *scale,
            );
            if game.shock > 0.0 {
                for pixel in &mut frame.pixels {
                    pixel.0 = pixel.0.saturating_add((game.shock * 28.0) as u8);
                    pixel.1 = (pixel.1 as f32 * 0.62) as u8;
                    pixel.2 = (pixel.2 as f32 * 0.56) as u8;
                }
            }
            let footer = if game.dead() {
                "YOU DIED   ENTER / R RETRY   ESC MENU".into()
            } else if player.won {
                "ESCAPED   ENTER / ESC main menu".into()
            } else if !game.message.is_empty() {
                game.message.clone()
            } else {
                let mut text = crate::console::Buffer::default();
                text.bytes(b"HP ")
                    .padded_number(game.progress.health as usize, 3, b' ')
                    .bytes(b" / COINS ")
                    .number(game.coins.collected())
                    .bytes(b"/3 / [ ] vol / ")
                    .bytes(audio.status().as_bytes())
                    .bytes(b" / ")
                    .bytes(if game.running { b"RUN" } else { b"WALK" })
                    .bytes(b" / WASD / Z run / ESC pause");
                text.into_text()
            };
            terminal.present_status(
                &frame,
                &footer,
                Some((game.battery.charge, game.flashlight)),
            )?;
            if player.won {
                victory(&mut terminal, w, h, game.progress.seconds)?;
                audio.pause(true);
                crate::proof::save_prompt(&mut terminal, &mut recording, input.held)?;
                return Ok(Outcome { return_menu: true });
            }
            mesh.triangles.truncate(base);
        }
        let elapsed = started.elapsed();
        let target = Duration::from_millis(33);
        if elapsed < target {
            crate::runtime::sleep(target - elapsed);
        }
    }
}

fn victory(terminal: &mut Terminal, width: usize, height: usize, seconds: f32) -> Result<()> {
    let mut time = crate::console::Buffer::default();
    time.bytes(b"TIME  ")
        .padded_number(seconds as usize / 60, 2, b'0')
        .bytes(b":")
        .padded_number(seconds as usize % 60, 2, b'0');
    let time = time.into_text();
    let lines = [
        "YOU ESCAPED",
        "EXIT FOUND",
        "",
        time.as_str(),
        "",
        "ENTER / ESC  MAIN MENU",
    ];
    let panel_width = 28.min(width.saturating_sub(4));
    let left = (width - panel_width) / 2;
    let top = (height.saturating_sub(lines.len() + 2)) / 2;
    for (row, line) in lines.iter().enumerate() {
        let padding = panel_width.saturating_sub(line.len()) / 2;
        let mut text = " ".repeat(padding);
        text.push_str(line);
        text.extend(core::iter::repeat_n(
            ' ',
            panel_width.saturating_sub(text.len()),
        ));
        terminal.overlay(left, top + row, &text, row == 0)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classic_terminal_motion_expires_without_key_up() {
        let map = Map::generated(7, 1).unwrap();
        let mut player = Player::new(&map);
        let mut input = Input::new();
        input.movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE), false);
        for _ in 0..100 {
            input.update(&mut player, &map, 0.02, false);
        }
        let position = player.camera.position;
        for _ in 0..100 {
            input.update(&mut player, &map, 0.02, false);
        }
        assert_eq!(position, player.camera.position);
        assert!(player.distance > 0.0 && player.distance <= 0.181);
    }
    #[test]
    fn enhanced_terminal_key_release_stops_movement() {
        let map = Map::generated(7, 1).unwrap();
        let mut player = Player::new(&map);
        let mut input = Input::new();
        input.movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE), true);
        input.update(&mut player, &map, 0.02, true);
        assert!(player.distance > 0.0);
        let released = KeyEvent::new_with_kind(
            KeyCode::Char('w'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        input.movement(released, true);
        let position = player.camera.position;
        input.update(&mut player, &map, 0.02, true);
        assert_eq!(position, player.camera.position);
    }

    #[test]
    fn simultaneous_keys_remain_independent_and_diagonal_speed_is_bounded() {
        let mut map = Map::generated(17, 1).unwrap();
        for z in 1..16 {
            for x in 1..16 {
                map.set_wall(x, z, false);
            }
        }
        map.spawn.x = 15.0;
        map.spawn.z = 15.0;
        let mut player = Player::new(&map);
        let mut input = Input::new();
        input.run_override = true;
        input.movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::SHIFT), true);
        input.movement(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SHIFT), true);
        input.update(&mut player, &map, 0.1, true);
        assert!((player.distance - RUN_SPEED * 0.1).abs() < 0.001);
        input.movement(
            KeyEvent::new_with_kind(
                KeyCode::Char('w'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
            true,
        );
        let before = player.camera.position;
        input.update(&mut player, &map, 0.1, true);
        let delta = player.camera.position - before;
        assert!(delta.dot(player.camera.forward()).abs() < 0.001);
        assert!((delta.dot(player.camera.right()) - RUN_SPEED * 0.1).abs() < 0.001);
        input.clear();
        let before = player.camera.position;
        input.update(&mut player, &map, 0.1, true);
        assert_eq!(before, player.camera.position);
    }

    #[test]
    fn a_held_walk_survives_other_keys_and_needs_no_typematic_repeats() {
        let mut map = Map::generated(31, 1).unwrap();
        for z in 1..30 {
            for x in 1..30 {
                map.set_wall(x, z, false);
            }
        }
        map.spawn.x = 29.0;
        map.spawn.z = 29.0;
        let mut player = Player::new(&map);
        let mut input = Input::new();
        input.movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE), true);
        for _ in 0..25 {
            input.update(&mut player, &map, 0.02, true);
        }
        assert!((player.distance - WALK_SPEED * 0.5).abs() < 0.001);
        assert!(input.action(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE), true));
        input.movement(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), true);
        let yaw = player.camera.yaw;
        let distance = player.distance;
        for _ in 0..25 {
            input.update(&mut player, &map, 0.02, true);
        }
        assert!(player.camera.yaw != yaw);
        assert!((player.distance - distance - WALK_SPEED * 0.5).abs() < 0.001);
        input.movement(
            KeyEvent::new_with_kind(KeyCode::Right, KeyModifiers::NONE, KeyEventKind::Release),
            true,
        );
        input.action(
            KeyEvent::new_with_kind(
                KeyCode::Char('f'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
            true,
        );
        let distance = player.distance;
        input.update(&mut player, &map, 0.1, true);
        assert!((player.distance - distance - WALK_SPEED * 0.1).abs() < 0.001);
        input.movement(
            KeyEvent::new_with_kind(
                KeyCode::Char('w'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
            true,
        );
        let position = player.camera.position;
        input.update(&mut player, &map, 0.1, true);
        assert_eq!(position, player.camera.position);
    }

    #[test]
    fn action_edges_ignore_windows_repeated_presses_and_protocol_repeats() {
        let mut input = Input::new();
        let mute = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE);
        assert!(input.action(mute, true));
        assert!(!input.action(mute, true));
        input.stop_motion();
        assert!(
            !input.action(mute, true),
            "death freeze must preserve the held mute edge"
        );
        let press = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(input.action(press, true));
        for _ in 0..40 {
            assert!(!input.action(press, true));
        }
        assert!(!input.action(
            KeyEvent::new_with_kind(KeyCode::Char('f'), KeyModifiers::NONE, KeyEventKind::Repeat),
            true
        ));
        assert!(!input.action(
            KeyEvent::new_with_kind(
                KeyCode::Char('f'),
                KeyModifiers::NONE,
                KeyEventKind::Release
            ),
            true
        ));
        assert!(input.action(press, true));
        input.clear();
        assert!(input.action(press, true));
    }
}
