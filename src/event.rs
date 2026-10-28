//! Minimal console events: native Windows key-up/down, VT/Kitty on Unix.
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::prelude::*;
#[cfg(not(windows))]
use crate::runtime::Instant;
#[cfg(not(windows))]
use alloc::collections::VecDeque;
use core::time::Duration;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyModifiers(u8);
impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}
impl KeyEvent {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self::new_with_kind(code, modifiers, KeyEventKind::Press)
    }
    pub fn new_with_kind(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Self {
        Self {
            code,
            modifiers,
            kind,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    Resize(u16, u16),
    FocusLost,
    FocusGained,
}
unsafe extern "C" {
    fn halls_term_open() -> i32;
    fn halls_term_close();
    fn halls_term_size(w: *mut i32, h: *mut i32) -> i32;
    #[cfg(not(windows))]
    fn halls_term_bytes(data: *mut u8, capacity: i32, timeout: i32) -> i32;
    #[cfg(windows)]
    fn halls_term_event(event: *mut Native, timeout: i32) -> i32;
}
#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Native {
    kind: i32,
    code: i32,
    modifiers: i32,
    event_kind: i32,
    width: i32,
    height: i32,
}
fn failure() -> Error {
    Error::new("Console unavailable".into())
}
pub fn open() -> Result<()> {
    if unsafe { halls_term_open() } != 0 {
        return Err(failure());
    }
    state(|s| *s = State::default());
    Ok(())
}
pub fn close() {
    unsafe {
        halls_term_close();
    }
}
pub fn size() -> Result<(u16, u16)> {
    let (mut w, mut h) = (0, 0);
    if unsafe { halls_term_size(&mut w, &mut h) } != 0 {
        return Err(failure());
    }
    Ok((w.max(1) as u16, h.max(1) as u16))
}
fn code(n: u32) -> Option<KeyCode> {
    Some(match n {
        13 | 10 => KeyCode::Enter,
        27 => KeyCode::Esc,
        57350 => KeyCode::Left,
        57351 => KeyCode::Right,
        57352 => KeyCode::Up,
        57353 => KeyCode::Down,
        _ => KeyCode::Char(char::from_u32(n)?),
    })
}
#[derive(Default)]
struct State {
    #[cfg(not(windows))]
    events: VecDeque<Event>,
    #[cfg(windows)]
    event: Option<Event>,
    #[cfg(not(windows))]
    parser: Parser,
    #[cfg(not(windows))]
    dimensions: (u16, u16),
}
// Serialized access also makes the public polling API safe across callers.
fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    use core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering},
    };
    struct Global(UnsafeCell<Option<State>>);
    // Every access is protected by LOCK; no references escape the closure.
    unsafe impl Sync for Global {}
    static STATE: Global = Global(UnsafeCell::new(None));
    static LOCK: AtomicBool = AtomicBool::new(false);
    struct Unlock;
    impl Drop for Unlock {
        fn drop(&mut self) {
            LOCK.store(false, Ordering::Release);
        }
    }
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    let _guard = Unlock;
    unsafe { f((&mut *STATE.0.get()).get_or_insert_with(State::default)) }
}

impl State {
    fn ready(&self) -> bool {
        #[cfg(windows)]
        {
            self.event.is_some()
        }
        #[cfg(not(windows))]
        {
            !self.events.is_empty()
        }
    }
    fn pop(&mut self) -> Option<Event> {
        #[cfg(windows)]
        {
            self.event.take()
        }
        #[cfg(not(windows))]
        {
            self.events.pop_front()
        }
    }
    fn pump(&mut self, timeout: Duration) -> Result<bool> {
        if self.ready() {
            return Ok(true);
        }
        #[cfg(windows)]
        {
            let mut native = Native::default();
            let n = unsafe {
                halls_term_event(
                    &mut native,
                    timeout.as_millis().min(i32::MAX as u128) as i32,
                )
            };
            if n < 0 {
                return Err(failure());
            }
            if n == 0 {
                return Ok(false);
            }
            let event = match native.kind {
                1 => {
                    let code = match native.code {
                        256 => Some(KeyCode::Up),
                        257 => Some(KeyCode::Down),
                        258 => Some(KeyCode::Left),
                        259 => Some(KeyCode::Right),
                        n => code(n as u32),
                    };
                    code.map(|code| {
                        Event::Key(KeyEvent {
                            code,
                            modifiers: KeyModifiers(native.modifiers as u8),
                            kind: if native.event_kind == 2 {
                                KeyEventKind::Release
                            } else {
                                KeyEventKind::Press
                            },
                        })
                    })
                }
                2 => Some(Event::Resize(native.width as u16, native.height as u16)),
                3 => Some(Event::FocusLost),
                4 => Some(Event::FocusGained),
                _ => None,
            };
            self.event = event;
        }
        #[cfg(not(windows))]
        {
            let now = size()?;
            if self.dimensions != now {
                self.dimensions = now;
                self.events.push_back(Event::Resize(now.0, now.1));
                return Ok(true);
            }
            self.parser.parse(&mut self.events);
            if self.ready() {
                return Ok(true);
            }
            let timeout = if self.parser.since.is_some() {
                timeout.min(Duration::from_millis(25))
            } else {
                timeout
            };
            let mut bytes = [0; 512];
            let n = unsafe {
                halls_term_bytes(
                    bytes.as_mut_ptr(),
                    bytes.len() as i32,
                    timeout.as_millis().min(i32::MAX as u128) as i32,
                )
            };
            if n < 0 {
                return Err(failure());
            }
            self.parser.bytes.extend(&bytes[..n as usize]);
            self.parser.parse(&mut self.events);
        }
        Ok(self.ready())
    }
}
pub fn poll(timeout: Duration) -> Result<bool> {
    state(|s| s.pump(timeout))
}
pub fn read() -> Result<Event> {
    loop {
        if poll(Duration::from_millis(100))? {
            return state(|s| s.pop().ok_or_else(failure));
        }
    }
}
pub fn negotiate() -> Result<bool> {
    if cfg!(windows) {
        return Ok(false);
    }
    #[cfg(not(windows))]
    {
        crate::console::text(b"\x1b[?u\x1b[c")?;
        let until = Instant::now() + Duration::from_millis(120);
        while Instant::now() < until {
            let mut bytes = [0; 512];
            let n = unsafe { halls_term_bytes(bytes.as_mut_ptr(), 512, 10) };
            if n < 0 {
                return Err(failure());
            }
            let ready = state(|s| {
                let State { parser, events, .. } = s;
                parser.bytes.extend(&bytes[..n as usize]);
                parser.parse(events);
                parser.enhanced
            });
            if ready {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(not(windows))]
#[derive(Default)]
struct Parser {
    bytes: Vec<u8>,
    since: Option<Instant>,
    paste: bool,
    enhanced: bool,
}
#[cfg(not(windows))]
impl Parser {
    fn parse(&mut self, events: &mut VecDeque<Event>) {
        loop {
            if self.bytes.is_empty() {
                self.since = None;
                return;
            }
            if self.paste {
                if let Some(end) = self.bytes.windows(6).position(|w| w == b"\x1b[201~") {
                    self.bytes.drain(..end + 6);
                    self.paste = false;
                    continue;
                }
                let excess = self.bytes.len().saturating_sub(5);
                self.bytes.drain(..excess);
                return;
            }
            if self.bytes[0] == 27 {
                let since = *self.since.get_or_insert_with(Instant::now);
                if self.bytes.len() == 1 {
                    if since.elapsed() < Duration::from_millis(25) {
                        return;
                    }
                    self.bytes.remove(0);
                    events.push_back(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
                    self.since = None;
                    continue;
                }
                if matches!(self.bytes[1], b'[' | b'O') {
                    let Some(end) = self
                        .bytes
                        .iter()
                        .enumerate()
                        .skip(2)
                        .find_map(|(i, &c)| (0x40..=0x7e).contains(&c).then_some(i))
                    else {
                        if self.bytes.len() > 128 || since.elapsed() > Duration::from_millis(100) {
                            self.bytes.clear();
                            self.since = None;
                        }
                        return;
                    };
                    let seq = self.bytes[2..=end].to_vec();
                    self.bytes.drain(..=end);
                    self.since = None;
                    self.sequence(&seq, events);
                    continue;
                }
                self.bytes.remove(0);
                self.since = None;
                // Preserve Escape before an unrelated key, rather than losing menu input.
                events.push_back(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
                continue;
            }
            let b = self.bytes[0];
            if !b.is_ascii() {
                let width = match b {
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => {
                        self.bytes.remove(0);
                        continue;
                    }
                };
                match core::str::from_utf8(&self.bytes[..width.min(self.bytes.len())]) {
                    Ok(text) => {
                        let character = text.chars().next().unwrap();
                        self.bytes.drain(..width);
                        events.push_back(Event::Key(KeyEvent::new(
                            KeyCode::Char(character),
                            KeyModifiers::NONE,
                        )));
                    }
                    Err(error) if error.error_len().is_none() => return,
                    Err(_) => {
                        self.bytes.remove(0);
                    }
                }
                continue;
            }
            self.bytes.remove(0);
            let mut modifiers = KeyModifiers::NONE;
            let value = if (1..=26).contains(&b) && !matches!(b, 9 | 10 | 13) {
                modifiers = KeyModifiers::CONTROL;
                (b + b'a' - 1) as u32
            } else {
                b as u32
            };
            if b.is_ascii_uppercase() {
                modifiers = KeyModifiers::SHIFT;
            }
            if let Some(code) = code(value) {
                events.push_back(Event::Key(KeyEvent::new(code, modifiers)));
            }
        }
    }
    fn sequence(&mut self, seq: &[u8], events: &mut VecDeque<Event>) {
        if seq == b"200~" {
            self.paste = true;
            return;
        }
        if seq == b"I" {
            events.push_back(Event::FocusGained);
            return;
        }
        if seq == b"O" {
            events.push_back(Event::FocusLost);
            return;
        }
        let Some((&last, body)) = seq.split_last() else {
            return;
        };
        if body.first() == Some(&b'?') {
            if last == b'u' {
                self.enhanced = true;
            }
            return;
        }
        let Ok(body) = core::str::from_utf8(body) else {
            return;
        };
        let mut fields = body.split(';');
        let primary = fields
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .parse::<u32>()
            .unwrap_or(0);
        let mut mods = fields.next().unwrap_or("1").split(':');
        let mods_num = mods
            .next()
            .unwrap_or("1")
            .parse::<u8>()
            .unwrap_or(1)
            .saturating_sub(1);
        let kind = match mods.next() {
            Some("2") => KeyEventKind::Repeat,
            Some("3") => KeyEventKind::Release,
            _ => KeyEventKind::Press,
        };
        let modifiers =
            KeyModifiers((mods_num & 1) | ((mods_num & 4) >> 1) | ((mods_num & 2) << 1));
        let key = match last {
            b'A' => Some(KeyCode::Up),
            b'B' => Some(KeyCode::Down),
            b'C' => Some(KeyCode::Right),
            b'D' => Some(KeyCode::Left),
            b'u' => code(primary),
            _ => None,
        };
        if let Some(code) = key {
            events.push_back(Event::Key(KeyEvent::new_with_kind(code, modifiers, kind)));
        }
    }
}
#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    #[test]
    fn utf8_input_survives_split_reads_and_invalid_prefixes() {
        let mut parser = Parser::default();
        let mut events = VecDeque::new();
        for character in ['λ', '胜', '😀'] {
            let mut encoded = [0; 4];
            let bytes = character.encode_utf8(&mut encoded).as_bytes();
            for (i, &byte) in bytes.iter().enumerate() {
                parser.bytes.push(byte);
                parser.parse(&mut events);
                if i + 1 < bytes.len() {
                    assert!(events.is_empty());
                }
            }
            assert_eq!(
                events.pop_front(),
                Some(Event::Key(KeyEvent::new(
                    KeyCode::Char(character),
                    KeyModifiers::NONE
                )))
            );
        }
        parser.bytes.extend(b"\xff\x80\xf0w\xc0\xaf\xed\xa0\x80\r");
        parser.parse(&mut events);
        assert_eq!(
            events.into_iter().collect::<Vec<_>>(),
            vec![
                Event::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)),
                Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            ]
        );
        assert!(parser.bytes.is_empty());
    }

    #[test]
    fn fragmented_csi_and_key_releases() {
        let mut p = Parser::default();
        let mut events = VecDeque::new();
        p.bytes.extend(b"\x1b[119;1");
        p.parse(&mut events);
        assert!(events.is_empty());
        p.bytes.extend(b":3u\x1b[1;2C\x1b[102;1:2u");
        p.parse(&mut events);
        assert_eq!(
            events.pop_front(),
            Some(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('w'),
                KeyModifiers::NONE,
                KeyEventKind::Release
            )))
        );
        assert_eq!(
            events.pop_front(),
            Some(Event::Key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::SHIFT
            )))
        );
        assert!(matches!(
            events.pop_front(),
            Some(Event::Key(KeyEvent {
                kind: KeyEventKind::Repeat,
                ..
            }))
        ));
    }
    #[test]
    fn paste_is_not_game_input_and_focus_is_preserved() {
        let mut p = Parser::default();
        let mut events = VecDeque::new();
        p.bytes.extend(b"\x1b[200~wwwwfr");
        p.parse(&mut events);
        assert!(events.is_empty());
        p.bytes.extend(b"\x1b[201~\x1b[O\x1b[I\x1b[?31uw");
        p.parse(&mut events);
        assert!(p.enhanced);
        assert_eq!(events.pop_front(), Some(Event::FocusLost));
        assert_eq!(events.pop_front(), Some(Event::FocusGained));
        assert_eq!(events.len(), 1);
    }
}
