//! Lossless per-tick recording. No disk I/O occurs until a completed run is exported.
#[allow(unused_imports)]
use crate::prelude::*;
use crate::{game::Game, player::Player, world::Map};

use crate::record_cipher::{Cipher, wipe};
#[path = "proof_stream.rs"]
mod stream;

const BASE_WORDS: usize = 24;
const CLOCK_WORDS: usize = 10;
const ENTITY_WORDS: usize = 12;
const BATTERY_WORDS: usize = 3;
const EXIT_WORDS: usize = 2;
unsafe extern "C" {
    fn halls_proof_time(out: *mut u8);
    fn halls_proof_clock(out: *mut u64);
    fn halls_proof_write(name: *const u8, data: *const u8, size: u32) -> i32;
}

pub struct Recording {
    data: Vec<u8>,
    cipher: Cipher,
    #[cfg(any(feature = "tools", test))]
    raw_cache: Vec<u8>,
    previous: Vec<u32>,
    frames: u32,
    complete: bool,
    origin: [u64; 3],
    clock: [u64; 3],
    raw_ns: u64,
    paused_us: u64,
    paused_at: Option<u64>,
    flashlight_input: bool,
}
impl Recording {
    #[inline(always)]
    pub fn new(map: &Map, game: &Game, player: &Player) -> Self {
        let origin = Self::read_clock();
        let mut data =
            Vec::with_capacity(88 + game.progress.entities.len() * 8 + map.walls.bits.len() * 8);
        data.extend_from_slice(b"HLR5");
        data.extend_from_slice(&0x0100_0001u32.to_le_bytes());
        for n in [map.width() as u16, map.height() as u16] {
            data.extend_from_slice(&n.to_le_bytes());
        }
        data.push(game.progress.entities.len() as u8);
        data.push(
            1 | ((cfg!(target_os = "macos") as u8 + 2 * cfg!(target_os = "linux") as u8) << 2)
                | ((cfg!(target_arch = "aarch64") as u8) << 4)
                | if cfg!(feature = "tools") { 128 } else { 0 },
        ); // Mark developer recordings.
        let words = BASE_WORDS
            + ENTITY_WORDS * game.progress.entities.len()
            + BATTERY_WORDS
            + CLOCK_WORDS
            + EXIT_WORDS;
        data.extend_from_slice(&(words as u16).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&map.seed.to_le_bytes());
        data.extend_from_slice(&crate::world::fresh_seed().to_le_bytes());
        for n in map.exit {
            data.extend_from_slice(&u16::try_from(n).unwrap_or(u16::MAX).to_le_bytes());
        }
        for n in [map.tile_size, map.ceiling, map.eye_height] {
            data.extend_from_slice(&n.to_le_bytes());
        }
        data.extend_from_slice(&((map.walls.bits.len() * 8) as u32).to_le_bytes());
        data.extend_from_slice(&[0u8; 12]); // UTC Unix seconds + signed local offset minutes.
        data.extend_from_slice(&origin[1].to_le_bytes()); // UTC start, microseconds.
        data.extend_from_slice(&0u64.to_le_bytes()); // UTC finish; stamped at victory.
        data.extend_from_slice(
            &(if cfg!(target_os = "windows") {
                1u32
            } else if cfg!(target_os = "macos") {
                2u32
            } else {
                3u32
            })
            .to_le_bytes(),
        );
        for e in &game.progress.entities {
            data.extend_from_slice(&e.form_seed.to_le_bytes());
        }
        for bits in &map.walls.bits {
            data.extend_from_slice(&bits.to_le_bytes());
        }
        let cipher = Cipher::new();
        cipher.bytes(&mut data, 0);
        let mut previous = vec![0; words];
        cipher.words(&mut previous, 0);
        let mut record = Self {
            data,
            cipher,
            #[cfg(any(feature = "tools", test))]
            raw_cache: Vec::new(),
            previous,
            frames: 0,
            complete: false,
            origin,
            clock: origin,
            raw_ns: 0,
            paused_us: 0,
            paused_at: None,
            flashlight_input: false,
        };
        record.tick(map, game, player, 0., [0.; 4], 0);
        record
    }
    fn read_clock() -> [u64; 3] {
        let mut clock = [0; 3];
        unsafe { halls_proof_clock(clock.as_mut_ptr()) };
        clock
    }
    pub fn begin_frame(&mut self, raw_ns: u64) {
        self.clock = Self::read_clock();
        self.raw_ns = raw_ns;
    }
    pub fn pause(&mut self) {
        if self.paused_at.is_none() {
            self.paused_at = Some(Self::read_clock()[0]);
        }
    }
    pub fn resume(&mut self) -> bool {
        if let Some(start) = self.paused_at.take() {
            let now = Self::read_clock();
            self.paused_us += now[0].saturating_sub(start);
            self.clock = now;
            true
        } else {
            false
        }
    }
    pub fn input_protocol(&mut self, releases: bool) {
        if !releases {
            let mut flag = [self.data[13]];
            self.cipher.bytes(&mut flag, 13);
            flag[0] |= 2;
            self.patch(13, &mut flag);
        }
    }
    pub fn flashlight_input(&mut self) {
        self.flashlight_input = !self.flashlight_input;
    }
    #[inline(always)]
    pub fn tick(
        &mut self,
        map: &Map,
        game: &Game,
        player: &Player,
        dt: f32,
        action: [f32; 4],
        held: u8,
    ) {
        if self.complete {
            return;
        }
        #[cfg(any(feature = "tools", test))]
        {
            wipe(&mut self.raw_cache);
            self.raw_cache.clear();
        }
        let camera = game.visual_camera(player);
        let mut words = Vec::with_capacity(self.previous.len());
        words.push(dt.to_bits());
        words.push(
            held as u32
                | ((game.running as u32) << 8)
                | ((game.flashlight as u32) << 9)
                | ((self.flashlight_input as u32) << 10),
        );
        self.flashlight_input = false;
        for x in action {
            words.push(x.to_bits());
        }
        for x in [
            player.camera.position.x,
            player.camera.position.y,
            player.camera.position.z,
            player.camera.yaw,
            player.camera.pitch,
            player.distance,
            camera.position.x,
            camera.position.y,
            camera.position.z,
            camera.yaw,
            camera.pitch,
            camera.roll,
            camera.fov,
            game.progress.health,
            game.knocked,
            game.shock,
            game.progress.seconds,
        ] {
            words.push(x.to_bits());
        }
        words.push(player.won as u32 | ((game.dead() as u32) << 1));
        for e in &game.progress.entities {
            for x in [
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
            ] {
                words.push(x.to_bits());
            }
            words.push(e.mood as u32);
        }
        words.extend_from_slice(&[
            game.battery.charge.to_bits(),
            (game.battery.age as f32).to_bits(),
            game.battery.wanted as u32,
        ]);
        for value in [
            self.clock[0].saturating_sub(self.origin[0]),
            self.clock[1],
            self.raw_ns,
            self.paused_us,
            self.clock[2].saturating_sub(self.origin[2]),
        ] {
            words.push(value as u32);
            words.push((value >> 32) as u32);
        }
        // Observe the current map field at each tick even in an inlined release
        // build. The live Map borrow makes this aligned pointer valid. Volatile
        // sampling is not a claim that a modified client cannot forge a record.
        let exit = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(map.exit)) };
        for coordinate in exit {
            // Keep an invalid sentinel rather than wrapping an out-of-range
            // address-sized value back into an apparently valid map cell.
            words.push(u32::try_from(coordinate).unwrap_or(u32::MAX));
        }
        let mut previous = self.previous.clone();
        self.cipher.words(&mut previous, self.frames);
        let mut packet = Vec::with_capacity(words.len() * 5 + words.len().div_ceil(8));
        for (group, old) in words.chunks(8).zip(previous.chunks_mut(8)) {
            let mask_at = packet.len();
            packet.push(0);
            for (i, (&value, prev)) in group.iter().zip(old).enumerate() {
                let mut delta = value ^ *prev;
                *prev = value;
                if delta != 0 {
                    packet[mask_at] |= 1 << i;
                    while delta >= 128 {
                        packet.push(delta as u8 | 128);
                        delta >>= 7;
                    }
                    packet.push(delta as u8);
                }
            }
        }
        self.cipher.bytes(&mut packet, self.data.len());
        self.data.extend_from_slice(&packet);
        wipe(&mut previous);
        self.cipher.words(&mut words, self.frames + 1);
        self.previous = words;
        self.frames += 1;
        self.complete = player.won && !game.dead() && self.frames > 1;
        if self.complete {
            let mut stamp = [0u8; 12];
            unsafe { halls_proof_time(stamp.as_mut_ptr()) };
            stamp[..8].copy_from_slice(&(self.clock[1] / 1_000_000).to_le_bytes());
            self.patch(56, &mut stamp);
            self.patch(76, &mut self.clock[1].to_le_bytes());
        }
    }
    /// Non-winning runs cannot produce an authenticated export.
    #[inline(always)]
    pub fn export(&mut self, name: &str) -> core::result::Result<(), i32> {
        if !self.complete || !valid_name(name) {
            return Err(-1);
        }
        if self.data.len() > u32::MAX as usize - 64 {
            return Err(-1);
        }
        self.patch(16, &mut self.frames.to_le_bytes());
        let mut signature = stream::signature(&self.data, &self.cipher)?;
        let output_cipher = Cipher::new();
        let compressed = stream::pack(&self.data, &self.cipher, &signature, &output_cipher);
        wipe(&mut signature);
        let sealed = stream::seal(&compressed, &output_cipher, (self.data.len() + 64) as u32)?;
        let mut filename = name.as_bytes().to_vec();
        if !name.ends_with(".hproof") {
            filename.extend_from_slice(b".hproof");
        }
        filename.push(0);
        let result =
            unsafe { halls_proof_write(filename.as_ptr(), sealed.as_ptr(), sealed.len() as u32) };
        if result == 0 { Ok(()) } else { Err(result) }
    }
    #[inline(always)]
    fn patch(&mut self, offset: usize, bytes: &mut [u8]) {
        self.cipher.bytes(bytes, offset);
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
        wipe(bytes);
    }
    #[cfg(any(feature = "tools", test))]
    pub fn words_for_test(&self) -> Vec<u32> {
        let mut words = self.previous.clone();
        self.cipher.words(&mut words, self.frames);
        words
    }
    #[cfg(any(feature = "tools", test))]
    pub fn raw(&mut self) -> &[u8] {
        wipe(&mut self.raw_cache);
        self.raw_cache.clear();
        self.raw_cache.extend_from_slice(&self.data);
        self.cipher.bytes(&mut self.raw_cache, 0);
        self.raw_cache[16..20].copy_from_slice(&self.frames.to_le_bytes());
        &self.raw_cache
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        wipe(&mut self.data);
        wipe(&mut self.previous);
        #[cfg(any(feature = "tools", test))]
        wipe(&mut self.raw_cache);
    }
}
struct Plain(Vec<u8>);
impl Drop for Plain {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 160 || matches!(bytes.last(), Some(b'.' | b' ')) {
        return false;
    }
    let end = bytes.iter().position(|&b| b == b'.').unwrap_or(bytes.len());
    let base = &bytes[..end];
    let reserved = base.eq_ignore_ascii_case(b"CON")
        || base.eq_ignore_ascii_case(b"PRN")
        || base.eq_ignore_ascii_case(b"AUX")
        || base.eq_ignore_ascii_case(b"NUL")
        || base.eq_ignore_ascii_case(b"CONIN$")
        || base.eq_ignore_ascii_case(b"CONOUT$")
        || (base.len() == 4
            && (base[..3].eq_ignore_ascii_case(b"COM") || base[..3].eq_ignore_ascii_case(b"LPT"))
            && base[3].is_ascii_digit());
    // Unicode C0 and C1 controls have these fixed UTF-8 encodings. All forbidden
    // filename punctuation is ASCII, so the remaining Unicode bytes pass through.
    !reserved
        && !bytes.iter().enumerate().any(|(i, &b)| {
            b < 32
                || b == 127
                || matches!(
                    b,
                    b'\\' | b'/' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
                )
                || b == 0xc2
                    && bytes
                        .get(i + 1)
                        .is_some_and(|&b| (0x80..=0x9f).contains(&b))
        })
}

/// HLC1: 8-token groups; flag=1 denotes a 2-byte (11-bit distance, 5-bit length) match.
/// Window 2048, match length 3..34. The signed plain length terminates the stream.
#[cfg(any(feature = "tools", test))]
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut table = vec![usize::MAX; 4096];
    // Literal-only HLC is the largest output: one flag per eight input bytes.
    // Avoid reallocating and leaving freed copies of export plaintext in the heap.
    let capacity = input
        .len()
        .checked_add(input.len().div_ceil(8))
        .expect("Proof compression size");
    let mut out = Vec::with_capacity(capacity);
    let mut at = 0;
    while at < input.len() {
        let flag = out.len();
        out.push(0);
        for bit in 0..8 {
            if at == input.len() {
                break;
            }
            let mut length = 0;
            let mut distance = 0;
            if at + 2 < input.len() {
                let hash = (input[at] as usize * 251
                    + input[at + 1] as usize * 31
                    + input[at + 2] as usize)
                    & 4095;
                let from = table[hash];
                table[hash] = at;
                if from < at && at - from <= 2048 {
                    while length < 34
                        && at + length < input.len()
                        && input[from + length] == input[at + length]
                    {
                        length += 1;
                    }
                    distance = at - from;
                }
            }
            if length >= 3 {
                out[flag] |= 1 << bit;
                out.extend_from_slice(
                    &((((distance - 1) << 5) | (length - 3)) as u16).to_le_bytes(),
                );
                at += length;
            } else {
                out.push(input[at]);
                at += 1;
            }
        }
    }
    out
}
/// Runs only after victory. A failed save keeps the in-memory recording available.
#[inline(always)]
pub fn save_prompt(
    terminal: &mut crate::terminal::Terminal,
    record: &mut Recording,
    mut blocked: u8,
) -> crate::error::Result<()> {
    use crate::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    let mut name = String::new();
    let executable = crate::runtime::executable_name();
    let mut status = "Name your victory proof";
    let mut saved = false;
    let mut enter_held = false;
    loop {
        let mut screen = String::from("\r\n  HALLS / ESCAPED\r\n\r\n  ");
        screen.push_str(status);
        screen.push_str("\r\n\r\n  > ");
        screen.push_str(&name);
        if !name.ends_with(".hproof") {
            screen.push_str(".hproof");
        }
        if saved {
            if executable.is_empty() {
                screen.push_str("\r\n\r\n  Saved in the program directory.");
            } else {
                screen.push_str("\r\n\r\n  Saved beside ");
                screen.push_str(&executable);
                screen.push('.');
            }
            screen.push_str(" ENTER / ESC: main menu\r\n");
        } else {
            screen.push_str("\r\n\r\n  ENTER: save   BACKSPACE: edit   ESC: skip export\r\n");
        }
        terminal.text_screen(&screen)?;
        let next_event = event::read()?;
        let Event::Key(key) = next_event else {
            continue;
        };
        let bit = match key.code {
            KeyCode::Char('w' | 'W') => 1,
            KeyCode::Char('s' | 'S') => 2,
            KeyCode::Char('a' | 'A') => 4,
            KeyCode::Char('d' | 'D') => 8,
            KeyCode::Left => 16,
            KeyCode::Right => 32,
            KeyCode::Up => 64,
            KeyCode::Down => 128,
            _ => 0,
        };
        if key.kind == KeyEventKind::Release {
            if key.code == KeyCode::Enter {
                enter_held = false;
            }
            blocked &= !bit;
            continue;
        }
        if blocked & bit != 0 {
            continue;
        }
        if key.code == KeyCode::Esc {
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            continue;
        }
        if key.code == KeyCode::Enter && terminal.key_releases() {
            if enter_held {
                continue;
            }
            enter_held = true;
        }
        match key.code {
            KeyCode::Enter if saved => return Ok(()),
            KeyCode::Enter => {
                if !valid_name(&name) {
                    status = "Use a filename, without a path";
                    continue;
                }
                terminal.text_screen("\r\n  Signing and encrypting proof...\r\n")?;
                match record.export(&name) {
                    Ok(()) => {
                        saved = true;
                        status = "VICTORY PROOF SAVED";
                    }
                    Err(17 | 80 | 183) => status = "File exists. Choose another name.",
                    Err(-2) => status = "Encryption failed. Press ENTER to retry.",
                    Err(_) => status = "Write failed. Check the game folder permissions.",
                }
            }
            KeyCode::Char('\u{8}' | '\u{7f}') if !saved => {
                name.pop();
            }
            KeyCode::Char(c) if !saved && !c.is_control() && name.len() + c.len_utf8() <= 160 => {
                name.push(c)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded_frames(raw: &[u8]) -> Vec<Vec<u32>> {
        let count = u32::from_le_bytes(raw[16..20].try_into().unwrap());
        let width = u16::from_le_bytes(raw[14..16].try_into().unwrap()) as usize;
        let bitmap_bytes = u32::from_le_bytes(raw[52..56].try_into().unwrap()) as usize;
        let mut at = 88 + raw[12] as usize * 8 + bitmap_bytes;
        let mut previous = vec![0u32; width];
        let mut decoded = Vec::new();
        for _ in 0..count {
            for group in previous.chunks_mut(8) {
                let mask = raw[at];
                at += 1;
                for (bit, word) in group.iter_mut().enumerate() {
                    if mask & (1 << bit) != 0 {
                        let mut delta = 0;
                        let mut shift = 0;
                        loop {
                            let byte = raw[at];
                            at += 1;
                            delta |= ((byte & 127) as u32) << shift;
                            if byte < 128 {
                                break;
                            }
                            shift += 7;
                        }
                        *word ^= delta;
                    }
                }
            }
            decoded.push(previous.clone());
        }
        assert_eq!(at, raw.len());
        decoded
    }

    #[test]
    fn exit_changes_are_recorded_per_tick_with_the_original_clock_and_encryption() {
        let mut map = Map::procedural(42).unwrap();
        let original = map.exit;
        let mut game = Game::new(&map);
        let player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        let initial_header = record.raw()[..88].to_vec();
        assert_eq!(
            u32::from_le_bytes(initial_header[4..8].try_into().unwrap()),
            0x0100_0001
        );
        assert_eq!(
            u16::from_le_bytes(initial_header[14..16].try_into().unwrap()),
            99
        );
        assert_eq!(
            &record.words_for_test()[97..99],
            &original.map(|v| v as u32)
        );
        for (index, exit) in [[3, 5], [19, 27], original].into_iter().enumerate() {
            map.exit = exit;
            let elapsed = (index as u64 + 1) * 25_000;
            record.clock = [
                record.origin[0] + elapsed,
                record.origin[1] + elapsed,
                record.origin[2] + elapsed,
            ];
            record.raw_ns = 25_000_000;
            game.progress.seconds += 0.025;
            record.tick(&map, &game, &player, 0.025, [0.; 4], 0);
            assert!(record.raw_cache.is_empty());
            let plain = record.words_for_test();
            assert_eq!(&plain[97..99], &exit.map(|v| v as u32));
            assert_eq!(plain[87] as u64 | ((plain[88] as u64) << 32), elapsed);
            assert_ne!(
                record.previous, plain,
                "the retained previous frame must stay encrypted"
            );
        }
        let raw = record.raw().to_vec();
        assert_eq!(
            &raw[36..40],
            &initial_header[36..40],
            "the header remains the opening snapshot"
        );
        assert_ne!(
            &record.data[..88],
            &raw[..88],
            "the retained recording header must stay encrypted"
        );
        let decoded = decoded_frames(&raw);
        assert_eq!(decoded.len(), 4);
        for (index, exit) in [original, [3, 5], [19, 27], original]
            .into_iter()
            .enumerate()
        {
            assert_eq!(&decoded[index][97..99], &exit.map(|v| v as u32));
            assert_eq!(
                decoded[index][87] as u64 | ((decoded[index][88] as u64) << 32),
                index as u64 * 25_000
            );
        }
    }

    #[test]
    fn invalid_exit_coordinates_do_not_wrap_into_valid_cells() {
        let mut map = Map::procedural(42).unwrap();
        let game = Game::new(&map);
        let player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        map.exit = [256, 65_536];
        record.tick(&map, &game, &player, 0.025, [0.; 4], 0);
        assert_eq!(&record.words_for_test()[97..99], &[256, 65_536]);
        map.exit = [usize::MAX, usize::MAX];
        record.tick(&map, &game, &player, 0.025, [0.; 4], 0);
        assert_eq!(&record.words_for_test()[97..99], &[u32::MAX, u32::MAX]);
        let mut initially_invalid = Recording::new(&map, &game, &player);
        assert_eq!(&initially_invalid.raw()[36..40], &[255; 4]);
    }
    #[test]
    fn encrypted_history_roundtrips_every_word_and_header_patches_are_idempotent() {
        let map = Map::procedural(42).unwrap();
        let mut game = Game::new(&map);
        let mut player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        let mut expected = vec![record.words_for_test()];
        record.input_protocol(false);
        let flag = record.raw()[13];
        record.input_protocol(false);
        assert_eq!(record.raw()[13], flag);
        for tick in 0..120 {
            player.camera.yaw = tick as f32 * 0.031;
            game.progress.seconds += 0.025;
            record.tick(&map, &game, &player, 0.025, [0.1, 0., 0.031, 0.], 1);
            expected.push(record.words_for_test());
            assert!(!record.data.windows(4).any(|w| w == b"HLR5"));
        }
        let first = record.raw().to_vec();
        assert_eq!(first, record.raw());
        assert_eq!(&first[..4], b"HLR5");
        assert_eq!(u32::from_le_bytes(first[16..20].try_into().unwrap()), 121);
        assert_eq!(decoded_frames(&first), expected);
        record.tick(&map, &game, &player, 0.025, [0.; 4], 0);
        assert!(record.raw_cache.is_empty());
        let other = Recording::new(&map, &game, &player);
        assert_ne!(&record.data[..88], &other.data[..88]);
    }
    #[test]
    fn names_cannot_escape_executable_directory() {
        for s in ["", "..", "../x", "x/y", "C:x", "x\\y", "x\n", "x.", "x "] {
            assert!(!valid_name(s));
        }
        assert!(valid_name("通关-2026"));
    }
    #[test]
    fn records_controls_hit_camera_and_every_entity_without_quantization() {
        let map = Map::procedural(42).unwrap();
        let mut game = Game::new(&map);
        let mut player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        game.running = true;
        game.flashlight = false;
        game.progress.health = 66.;
        game.progress.seconds = 42.;
        game.knocked = 0.18;
        game.shock = 0.3;
        player.camera.pitch = -0.75;
        player.camera.yaw = 3.5;
        player.distance = 123.25;
        for e in &mut game.progress.entities {
            e.hate = 97.;
            e.attack_left = 0.31;
            e.move_blend = 0.82;
            e.mood = crate::entity::Mood::Chase;
            e.replan = -0.3;
        }
        let action = [-0.1, 0.2, -0.01, 0.02];
        record.tick(&map, &game, &player, 0.05, action, 0xa5);
        let w = record.words_for_test();
        assert_eq!(w[1], 0x1a5);
        assert_eq!(&w[2..6], &action.map(f32::to_bits));
        assert_eq!(w[10], (-0.75_f32).to_bits());
        assert_eq!(w[11], 123.25_f32.to_bits());
        assert_eq!(w[19], 66_f32.to_bits());
        let camera = game.visual_camera(&player);
        assert_eq!(w[13], camera.position.y.to_bits());
        assert_eq!(w[17], camera.roll.to_bits());
        for i in 0..game.progress.entities.len() {
            let k = 24 + i * 12;
            assert_eq!(w[k + 5], 0.31_f32.to_bits());
            assert_eq!(w[k + 6], 97_f32.to_bits());
            assert_eq!(w[k + 10], (-0.3_f32).to_bits());
            assert_eq!(w[k + 11], 2);
        }
    }
    #[test]
    fn battery_and_switch_inputs_have_their_own_recorded_words() {
        let map = Map::procedural(42).unwrap();
        let mut game = Game::new(&map);
        let player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        assert_eq!(record.previous.len(), 99);
        assert_eq!(&record.words_for_test()[84..87], &[100f32.to_bits(), 0, 1]);
        record.flashlight_input();
        game.battery.toggle();
        game.battery.tick(&mut game.flashlight, 0.05);
        record.tick(&map, &game, &player, 0.05, [0.; 4], 0);
        assert_ne!(record.words_for_test()[1] & 1024, 0);
        assert_eq!(
            &record.words_for_test()[84..87],
            &[
                game.battery.charge.to_bits(),
                (game.battery.age as f32).to_bits(),
                0
            ]
        );
        record.tick(&map, &game, &player, 0.05, [0.; 4], 0);
        assert_eq!(record.words_for_test()[1] & 1024, 0);
    }
    #[test]
    fn compression_roundtrips_overlap_and_incompressible_bytes() {
        for data in [
            vec![],
            vec![0; 10000],
            (0..20000).map(|i| ((i * i + i / 7) % 251) as u8).collect(),
        ] {
            let packed = compress(&data);
            let mut output = Vec::new();
            let mut i = 0;
            while output.len() < data.len() {
                let flags = packed[i];
                i += 1;
                for bit in 0..8 {
                    if output.len() == data.len() {
                        break;
                    }
                    if flags & (1 << bit) == 0 {
                        output.push(packed[i]);
                        i += 1;
                    } else {
                        let token = u16::from_le_bytes([packed[i], packed[i + 1]]) as usize;
                        i += 2;
                        for _ in 0..(token & 31) + 3 {
                            let b = output[output.len() - (token >> 5) - 1];
                            output.push(b);
                        }
                    }
                }
            }
            assert_eq!(i, packed.len());
            assert_eq!(output, data);
        }
    }
    #[test]
    fn death_never_unlocks_export_and_victory_freezes_recording() {
        let map = Map::generated(7, 42).unwrap();
        let mut game = Game::new(&map);
        let mut player = Player::new(&map);
        let mut lost = Recording::new(&map, &game, &player);
        player.won = true;
        game.progress.health = 0.;
        lost.tick(&map, &game, &player, 0.02, [0.; 4], 0);
        assert_eq!(lost.export("lost"), Err(-1));
        player.won = false;
        game.progress.health = 100.;
        let mut won = Recording::new(&map, &game, &player);
        player.won = true;
        won.tick(&map, &game, &player, 0.02, [0.; 4], 1);
        assert!(won.complete);
        let before = won.data.clone();
        won.tick(&map, &game, &player, 0.02, [1.; 4], 255);
        assert_eq!(won.frames, 2);
        assert_eq!(won.data, before);
    }
    #[test]
    fn timestamp_is_captured_on_victory_and_frozen() {
        let map = Map::generated(7, 42).unwrap();
        let game = Game::new(&map);
        let mut player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        assert_eq!(&record.raw()[..4], b"HLR5");
        assert_eq!(&record.raw()[56..68], &[0; 12]);
        player.won = true;
        record.tick(&map, &game, &player, 0.02, [0.; 4], 0);
        let stamp = record.raw()[56..68].to_vec();
        let seconds = i64::from_le_bytes(stamp[..8].try_into().unwrap());
        let offset = i32::from_le_bytes(stamp[8..].try_into().unwrap());
        assert!(seconds > 1_700_000_000);
        assert!((-840..=840).contains(&offset));
        record.tick(&map, &game, &player, 0.02, [0.; 4], 0);
        assert_eq!(&record.raw()[56..68], stamp.as_slice());
    }
    #[test]
    fn losing_or_unfinished_recordings_cannot_export() {
        let map = Map::generated(7, 42).unwrap();
        let game = Game::new(&map);
        let player = Player::new(&map);
        let mut record = Recording::new(&map, &game, &player);
        assert_eq!(record.export("test"), Err(-1));
    }
}

#[cfg(test)]
mod compact_name_tests {
    use super::valid_name;
    fn reference(name: &str) -> bool {
        let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && base.as_bytes()[3].is_ascii_digit());
        !reserved
            && !name.is_empty()
            && name.len() <= 160
            && !name.ends_with(['.', ' '])
            && !name
                .chars()
                .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
            && name != "."
            && name != ".."
    }
    #[test]
    fn byte_validation_matches_reference_for_every_unicode_character() {
        for n in 0..=0x10ffff {
            if let Some(c) = char::from_u32(n) {
                let mut name = String::from("a");
                name.push(c);
                name.push_str(".hproof");
                assert_eq!(valid_name(&name), reference(&name), "U+{n:04X}");
            }
        }
        for root in [
            "CON",
            "PRN",
            "AUX",
            "NUL",
            "CONIN$",
            "CONOUT$",
            "COM0",
            "COM1",
            "COM9",
            "LPT0",
            "LPT1",
            "LPT9",
            "COMA",
            "LPTA",
            "CONSOLE",
            "",
            ".",
            "..",
            "trailing ",
            "trailing.",
        ] {
            for mask in 0..(1usize << root.len()) {
                let name: String = root
                    .chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if mask & (1 << i) != 0 {
                            c.to_ascii_lowercase()
                        } else {
                            c
                        }
                    })
                    .collect();
                for ext in ["", ".hproof", ".txt", " ", "."] {
                    let full = name.clone() + ext;
                    assert_eq!(valid_name(&full), reference(&full), "{full:?}");
                }
            }
        }
    }
}
