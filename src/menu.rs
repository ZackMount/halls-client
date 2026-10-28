use crate::error::Result;
use crate::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    mesh::Mesh,
    player::Player,
    render::{Frame, MAX_RENDER_SCALE, Renderer, Rgb},
    terminal::Terminal,
    world::Map,
};
use core::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    New,
    Quit,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    Resume,
    Menu,
    Quit,
}

fn logo(frame: &mut Frame) {
    let scale = if frame.width >= 70 && frame.height >= 40 {
        2
    } else {
        1
    };
    let left = (frame.width - 29 * scale) / 2;
    let top = (frame.height / 6).min(12);
    for i in 0..5 {
        for y in 0..7 {
            for x in 0..5 {
                let stroke = match i {
                    0 => x == 0 || x == 4 || y == 3,
                    1 => {
                        if y == 0 {
                            (1..=3).contains(&x)
                        } else {
                            x == 0 || x == 4 || y == 3
                        }
                    }
                    2 | 3 => x == 0 || y == 6,
                    _ => match y {
                        0 => x > 0,
                        1 | 2 => x == 0,
                        3 => (1..=3).contains(&x),
                        4 | 5 => x == 4,
                        _ => x < 4,
                    },
                };
                if !stroke {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = left + (i * 6 + x) * scale + dx;
                        let py = top + y * scale + dy;
                        if px < frame.width && py < frame.height {
                            frame.pixels[py * frame.width + px] = if y < 4 {
                                Rgb(216, 195, 121)
                            } else {
                                Rgb(158, 130, 67)
                            };
                        }
                    }
                }
            }
        }
    }
}
fn background(mut frame: Frame) -> Frame {
    for color in &mut frame.pixels {
        color.0 /= 3;
        color.1 /= 3;
        color.2 /= 3;
    }
    logo(&mut frame);
    frame
}
fn select(
    terminal: &mut Terminal,
    frame: &Frame,
    items: &[&str],
    footer: &str,
    mut selected: usize,
    mut scale: Option<&mut usize>,
) -> Result<usize> {
    let mut redraw = true;
    loop {
        let (w, h) = terminal.size()?;
        if w < 32 || h < 12 {
            if redraw {
                terminal.text_screen("Resize to at least 32x12.\r\nEsc quits.\r\n")?;
                redraw = false;
            }
        } else if redraw {
            terminal.invalidate()?;
            let footer = if scale.as_deref().is_some_and(|value| *value >= 4) {
                "WARNING: may run slowly"
            } else {
                footer
            };
            if w == frame.width && h - 1 == frame.height / 2 {
                terminal.present(frame, footer)?;
            } else {
                let mut text = String::from("\r\n  HALLS\r\n");
                text.push_str(footer);
                terminal.text_screen(&text)?;
            }
            let box_w = if scale.is_some() { 20 } else { (w - 4).min(30) };
            let left = (w - box_w) / 2;
            let top = (h / 2).max(5).min(h.saturating_sub(items.len() + 2));
            for (i, name) in items.iter().enumerate() {
                let prefix = if i == selected { "> " } else { "  " };
                let mut text = crate::console::Buffer::default();
                if let Some(value) = scale.as_deref() {
                    text.bytes(b"RESOLUTION  < ").number(*value).bytes(b"x >");
                } else {
                    text.bytes(prefix.as_bytes()).bytes(name.as_bytes());
                }
                let mut text = text.into_text();
                text.truncate(box_w);
                text.extend(core::iter::repeat_n(' ', box_w - text.len()));
                terminal.overlay(left, top + i, &text, i == selected)?;
            }
            redraw = false;
        }
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Resize(_, _) => redraw = true,
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Left | KeyCode::Right if scale.is_some() => {
                        let value = scale.as_deref_mut().unwrap();
                        *value = if key.code == KeyCode::Left {
                            (*value / 2).max(1)
                        } else {
                            (*value * 2).min(MAX_RENDER_SCALE)
                        };
                        redraw = true;
                    }
                    KeyCode::Up | KeyCode::Char('w') => {
                        selected = (selected + items.len() - 1) % items.len();
                        redraw = true;
                    }
                    KeyCode::Down | KeyCode::Char('s') => {
                        selected = (selected + 1) % items.len();
                        redraw = true;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        return Ok(selected);
                    }
                    KeyCode::Esc => {
                        return Ok(if items[0] == "RESUME" {
                            0
                        } else {
                            items.len() - 1
                        });
                    }
                    KeyCode::Char('q') => return Ok(items.len() - 1),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(items.len() - 1);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
fn settings(terminal: &mut Terminal, frame: &Frame, scale: &mut usize) -> Result<()> {
    select(terminal, frame, &[""], "", 0, Some(scale))?;
    Ok(())
}
pub fn start(map: &Map, scale: &mut usize) -> Result<Choice> {
    let mut terminal = Terminal::enter()?;
    let mesh = Mesh::from_map(map);
    let player = Player::new(map);
    let mut renderer = Renderer::default();
    loop {
        let (w, h) = terminal.size()?;
        let frame = background(renderer.render(
            &mesh,
            player.camera,
            w.max(32),
            h.saturating_sub(1).max(11) * 2,
        ));
        match select(
            &mut terminal,
            &frame,
            &["NEW GAME", "SETTINGS", "QUIT"],
            concat!("HALLS ", env!("CARGO_PKG_VERSION"), " / arrows + enter"),
            0,
            None,
        )? {
            0 => return Ok(Choice::New),
            1 => settings(&mut terminal, &frame, scale)?,
            _ => return Ok(Choice::Quit),
        }
    }
}
pub fn pause(terminal: &mut Terminal, frame: Frame, scale: &mut usize) -> Result<Pause> {
    let frame = background(frame);
    loop {
        let result = match select(
            terminal,
            &frame,
            &["RESUME", "SETTINGS", "MAIN MENU", "QUIT"],
            "PAUSED",
            0,
            None,
        )? {
            0 => Pause::Resume,
            1 => {
                settings(terminal, &frame, scale)?;
                continue;
            }
            2 => Pause::Menu,
            _ => Pause::Quit,
        };
        terminal.invalidate()?;
        return Ok(result);
    }
}
