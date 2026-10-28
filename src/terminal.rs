use crate::console::{self, Buffer};
use crate::error::Result;
use crate::event as terminal;
#[allow(unused_imports)]
use crate::prelude::*;
use crate::render::{Frame, Rgb};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    top: Rgb,
    bottom: Rgb,
}
pub struct Terminal {
    pub enhanced_keys: bool,
    out: Buffer,
    previous: Vec<Cell>,
    dimensions: (usize, usize),
}
pub fn restore(enhanced_keys: bool) {
    crate::runtime::protocol(false);
    let mut out = Buffer::default();
    if enhanced_keys {
        out.bytes(b"\x1b[<u");
    }
    out.bytes(b"\x1b[?2004l\x1b[?1004l\x1b[?25h\x1b[?1049l\x1b[0m");
    let _ = out.flush();
    terminal::close();
}
impl Terminal {
    /// Windows console events include key-up independently of Kitty/CSI-u.
    /// `enhanced_keys` only controls pushing/popping the terminal protocol.
    pub fn key_releases(&self) -> bool {
        cfg!(target_os = "windows") || self.enhanced_keys
    }

    pub fn enter() -> Result<Self> {
        terminal::open()?;
        let setup = (|| -> Result<bool> {
            console::text(b"\x1b[?1049h\x1b[?25l\x1b[?1004h\x1b[?2004h\x1b[2J")?;
            let enhanced = terminal::negotiate()?;
            if enhanced {
                console::text(b"\x1b[>11u")?;
            }
            Ok(enhanced)
        })();
        let enhanced_keys = match setup {
            Ok(value) => value,
            Err(err) => {
                restore(false);
                return Err(err);
            }
        };
        crate::runtime::protocol(enhanced_keys);
        Ok(Self {
            enhanced_keys,
            out: Buffer::default(),
            previous: vec![],
            dimensions: (0, 0),
        })
    }
    pub fn size(&self) -> Result<(usize, usize)> {
        let (w, h) = terminal::size()?;
        Ok((w as usize, h as usize))
    }
    pub fn invalidate(&mut self) -> Result<()> {
        self.previous.clear();
        self.out.bytes(b"\x1b[0m\x1b[2J\x1b[H");
        self.out.flush()?;
        Ok(())
    }
    pub fn text_screen(&mut self, text: &str) -> Result<()> {
        self.previous.clear();
        self.out
            .bytes(b"\x1b[0m\x1b[2J\x1b[H")
            .bytes(text.as_bytes())
            .bytes(b"\x1b[J");
        self.out.flush()?;
        Ok(())
    }
    pub fn overlay(&mut self, x: usize, y: usize, text: &str, selected: bool) -> Result<()> {
        self.cursor(x, y);
        self.out
            .bytes(b"\x1b[48;5;233m\x1b[38;5;")
            .number(if selected { 223 } else { 180 })
            .bytes(b"m")
            .bytes(text.as_bytes())
            .bytes(b"\x1b[0m");
        self.out.flush()?;
        Ok(())
    }
    fn cursor(&mut self, x: usize, y: usize) {
        self.out
            .bytes(b"\x1b[")
            .number(y + 1)
            .bytes(b";")
            .number(x + 1)
            .bytes(b"H");
    }
    pub fn present(&mut self, frame: &Frame, footer: &str) -> Result<()> {
        self.present_status(frame, footer, None)
    }
    pub fn present_status(
        &mut self,
        frame: &Frame,
        footer: &str,
        battery: Option<(f32, bool)>,
    ) -> Result<()> {
        let w = frame.width;
        let rows = frame.height / 2;
        if self.dimensions != (w, rows) || self.previous.len() != w * rows {
            self.invalidate()?;
            self.dimensions = (w, rows);
            self.previous = vec![
                Cell {
                    top: Rgb(255, 0, 255),
                    bottom: Rgb(255, 0, 255)
                };
                w * rows
            ];
        }
        let mut fg = None;
        let mut bg = None;
        for y in 0..rows {
            let mut contiguous = false;
            for x in 0..w {
                let top = quantize(frame.pixels[y * 2 * w + x]);
                let bottom = quantize(frame.pixels[(y * 2 + 1) * w + x]);
                let cell = Cell { top, bottom };
                let index = y * w + x;
                if self.previous[index] == cell {
                    contiguous = false;
                    continue;
                }
                if !contiguous {
                    self.cursor(x, y);
                    contiguous = true;
                }
                if bg != Some(bottom) {
                    color(&mut self.out, bottom, false);
                    bg = Some(bottom);
                }
                if top == bottom {
                    self.out.bytes(b" ");
                } else {
                    if fg != Some(top) {
                        color(&mut self.out, top, true);
                        fg = Some(top);
                    }
                    self.out.bytes("▀".as_bytes());
                }
                self.previous[index] = cell;
            }
        }
        // Each run specifies its colors; the footer resets the terminal's state every frame.
        let line: String = footer
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(w.saturating_sub(if battery.is_some() { 28 } else { 1 }))
            .collect();
        self.out.bytes(b"\x1b[0m");
        self.cursor(0, rows);
        self.out
            .bytes(b"\x1b[2K\x1b[38;5;245m")
            .bytes(line.as_bytes())
            .bytes(b"\x1b[0m");
        if let Some((charge, on)) = battery {
            self.cursor(w.saturating_sub(27), rows);
            self.out.bytes(b"\x1b[38;5;245m").bytes(if on {
                b"F lamp on  "
            } else {
                b"F lamp off "
            });
            let percent = charge.clamp(0.0, 100.0).ceil() as usize;
            let cells = percent.div_ceil(20);
            let shade = if cells >= 4 {
                114
            } else if cells >= 2 {
                222
            } else {
                203
            };
            self.cursor(w.saturating_sub(16), rows);
            // Five pixel bars: a narrow dark edge separates colored backgrounds.
            // Use VT block coverage, not font-shaped brackets or empty-cell dots.
            self.out.bytes(b"\x1b[38;5;233m");
            for i in 0..5 {
                self.out
                    .bytes(b"\x1b[48;5;")
                    .number(if i < cells { shade } else { 236 })
                    .bytes(b"m")
                    .bytes("▏ ".as_bytes());
            }
            self.out
                .bytes(b"\x1b[0m\x1b[38;5;")
                .number(shade)
                .bytes(b"m ")
                .padded_number(percent, 3, b' ')
                .bytes(b"%\x1b[0m");
        }
        self.out.flush()?;
        Ok(())
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.out.flush();
        restore(self.enhanced_keys);
    }
}
fn quantize(Rgb(r, g, b): Rgb) -> Rgb {
    let q = |v: u8| ((v as u16 / 6) * 6 + 3).min(255) as u8;
    Rgb(q(r), q(g), q(b))
}
fn color(out: &mut Buffer, c: Rgb, foreground: bool) {
    out.bytes(if foreground {
        b"\x1b[38;2;"
    } else {
        b"\x1b[48;2;"
    })
    .number(c.0 as usize)
    .bytes(b";")
    .number(c.1 as usize)
    .bytes(b";")
    .number(c.2 as usize)
    .bytes(b"m");
}
