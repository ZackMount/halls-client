//! Real-time procedural audio: no recordings, codecs or audio assets.
#[allow(unused_imports)]
use crate::prelude::*;
use crate::{game::Game, math::Vec3, player::Player, world::Map};
use alloc::sync::Arc;
use core::{
    ffi::c_void,
    sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
};
const RATE: f32 = 48_000.0;
const QUEUE: usize = 64;
const FOOT: u8 = 1;
const ENTITY: u8 = 2;
const METAL: u8 = 3;
const RAGE: u8 = 4;
const SWING: u8 = 5;
const HIT: u8 = 6;
const DEATH: u8 = 7;
const EXIT: u8 = 8;
const SWITCH: u8 = 9;
const COIN: u8 = 10;

struct Controls {
    gain: AtomicU32,
    threat: AtomicU32,
    pan: AtomicU32,
    ambience: AtomicU32,
    reset: AtomicU32,
    ending: AtomicU32,
    write: AtomicUsize,
    read: AtomicUsize,
    events: [AtomicU64; QUEUE],
}
impl Controls {
    fn new() -> Self {
        Self {
            gain: AtomicU32::new(0),
            threat: AtomicU32::new(0),
            pan: AtomicU32::new(0),
            ambience: AtomicU32::new(1.0f32.to_bits()),
            reset: AtomicU32::new(0),
            ending: AtomicU32::new(0),
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            events: core::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
    // One game-thread producer and one audio-thread consumer. Full queues drop new events.
    fn push(&self, kind: u8, gain: f32, pan: f32) {
        let write = self.write.load(Ordering::Relaxed);
        let next = (write + 1) % QUEUE;
        if next == self.read.load(Ordering::Acquire) {
            return;
        }
        let value = kind as u64
            | ((gain.clamp(0.0, 1.0) * 65535.0) as u64) << 8
            | (((pan.clamp(-1.0, 1.0) + 1.0) * 32767.0) as u64) << 24;
        self.events[write].store(value, Ordering::Relaxed);
        self.write.store(next, Ordering::Release);
    }
    fn pop(&self) -> Option<(u8, f32, f32)> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.write.load(Ordering::Acquire) {
            return None;
        }
        let value = self.events[read].load(Ordering::Relaxed);
        self.read.store((read + 1) % QUEUE, Ordering::Release);
        Some((
            value as u8,
            ((value >> 8) & 65535) as f32 / 65535.0,
            ((value >> 24) & 65535) as f32 / 32767.0 - 1.0,
        ))
    }
}
fn parameter(value: &AtomicU32) -> f32 {
    f32::from_bits(value.load(Ordering::Relaxed))
}
fn set(value: &AtomicU32, n: f32) {
    value.store(n.to_bits(), Ordering::Relaxed);
}

#[derive(Clone, Copy, Default)]
struct Voice {
    kind: u8,
    age: f32,
    duration: f32,
    gain: f32,
    pan: f32,
    phase: f32,
    filter: f32,
}
struct Synth {
    controls: Arc<Controls>,
    voices: [Voice; 16],
    noise: u64,
    table: [f32; 2048],
    phases: [f32; 7],
    air: [f32; 2],
    delay: Vec<[f32; 2]>,
    cursor: usize,
    dc_in: [f32; 2],
    dc_out: [f32; 2],
    gain: f32,
    threat: f32,
    pan: f32,
    ambience: f32,
    time: f64,
    bell: f32,
    generation: u32,
}
impl Synth {
    fn new(controls: Arc<Controls>, seed: u64) -> Self {
        Self {
            controls,
            voices: [Voice::default(); 16],
            noise: seed.max(1),
            table: core::array::from_fn(|i| {
                (i as u32 as f32 * core::f32::consts::TAU / 2048.0).sin()
            }),
            phases: [0.0; 7],
            air: [0.0; 2],
            delay: vec![[0.0; 2]; 16_381],
            cursor: 0,
            dc_in: [0.0; 2],
            dc_out: [0.0; 2],
            gain: 0.0,
            threat: 0.0,
            pan: 0.0,
            ambience: 1.0,
            time: 0.0,
            bell: 9.0,
            generation: 0,
        }
    }
    fn random(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 7;
        self.noise ^= self.noise << 17;
        (self.noise >> 40) as f32 / 8_388_608.0 - 1.0
    }
    fn trigger(&mut self, kind: u8, gain: f32, pan: f32) {
        let slot = self
            .voices
            .iter()
            .position(|v| v.age >= v.duration)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        (a.1.gain * (1.0 - a.1.age / a.1.duration))
                            .total_cmp(&(b.1.gain * (1.0 - b.1.age / b.1.duration)))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });
        let duration = match kind {
            FOOT => 0.18,
            ENTITY => 0.32,
            METAL => 4.5,
            RAGE => 1.4,
            SWING => 0.32,
            HIT => 0.65,
            DEATH => 2.8,
            EXIT => 4.0,
            COIN => 0.28,
            _ => 0.035,
        };
        self.voices[slot] = Voice {
            kind,
            age: 0.0,
            duration,
            gain,
            pan,
            phase: 0.0,
            filter: 0.0,
        };
    }
    fn render(&mut self, output: &mut [f32]) {
        let generation = self.controls.reset.load(Ordering::Acquire);
        if generation != self.generation {
            self.voices.fill(Voice::default());
            self.delay.fill([0.0; 2]);
            self.generation = generation;
            self.threat = 0.0;
            self.bell = 9.0;
            while self.controls.pop().is_some() {}
        }
        while let Some((kind, gain, pan)) = self.controls.pop() {
            self.trigger(kind, gain, pan);
        }
        // End-of-run cues have a separate latch: a simultaneous stream reset
        // must not discard the one death/exit event together with stale footsteps.
        let ending = self.controls.ending.swap(0, Ordering::AcqRel) as u8;
        if ending != 0 {
            self.voices.fill(Voice::default());
            self.trigger(ending, if ending == DEATH { 1.0 } else { 0.65 }, 0.0);
        }
        let gain = parameter(&self.controls.gain);
        // Once the fade reaches silence, discard tails and avoid denormal arithmetic.
        // Muted time must not leave an old attack waiting to play on resume.
        if gain == 0.0 && self.gain < 0.000001 {
            if self.gain != 0.0 {
                self.delay.fill([0.0; 2]);
            }
            self.gain = 0.0;
            self.voices.fill(Voice::default());
            self.dc_in = [0.0; 2];
            self.dc_out = [0.0; 2];
            output.fill(0.0);
            return;
        }
        let threat = parameter(&self.controls.threat);
        let pan = parameter(&self.controls.pan);
        let ambience = parameter(&self.controls.ambience);
        for frame in output.as_chunks_mut::<2>().0 {
            self.gain += (gain - self.gain) * 0.001;
            self.threat += (threat - self.threat) * 0.000025;
            self.pan += (pan - self.pan) * 0.0001;
            self.ambience += (ambience - self.ambience) * 0.00004;
            self.time += 1.0 / RATE as f64;
            self.bell -= 1.0 / RATE;
            if self.bell <= 0.0 {
                let r = self.random();
                self.bell = 18.0 + r * 7.0;
                self.trigger(METAL, 0.08 * self.ambience, r);
            }
            let slow = wave(
                &self.table,
                crate::runtime::fractional_time(self.time * 0.023),
            );
            let root = 36.708 + slow * 0.12;
            let frequencies = [
                49.83,
                50.17,
                100.0,
                root,
                root * 1.498,
                0.85 + self.threat * 1.4,
                root * 2.013,
            ];
            for (phase, frequency) in self.phases.iter_mut().zip(frequencies) {
                *phase = (*phase + frequency / RATE).fract();
            }
            let hum = (wave(&self.table, self.phases[0]) + wave(&self.table, self.phases[1]))
                * 0.022
                + wave(&self.table, self.phases[2]) * 0.008;
            let drone = wave(&self.table, self.phases[3]) * 0.024
                + wave(&self.table, self.phases[4]) * 0.011;
            let pulse = (wave(&self.table, self.phases[5]) * 0.5 + 0.5).powi(6);
            let pursuit = self.threat
                * (wave(&self.table, self.phases[6]) * 0.12
                    + pulse * wave(&self.table, self.phases[3]) * 0.25
                    + wave(&self.table, self.phases[6] * 2.0) * 0.08);
            let mut stereo = [0.0; 2];
            for (ch, sample) in stereo.iter_mut().enumerate() {
                let noise = self.random();
                self.air[ch] += (noise - self.air[ch]) * 0.012;
                *sample =
                    self.ambience * ((hum + drone) * (0.9 + slow * 0.1) + self.air[ch] * 0.13);
                *sample += pursuit
                    * if ch == 0 {
                        (1.0 - self.pan * 0.6) * 0.7
                    } else {
                        (1.0 + self.pan * 0.6) * 0.7
                    };
            }
            let mut send = stereo;
            let noise = self.random();
            for voice in &mut self.voices {
                if voice.age >= voice.duration {
                    continue;
                }
                let t = voice.age / voice.duration;
                let attack = (voice.age / 0.005).min(1.0);
                let envelope = if voice.kind == COIN {
                    // The two electronic notes each have their own attack and
                    // decay, with a brief silence separating their syllables.
                    let (age, duration) = if voice.age < 0.10 {
                        (voice.age, 0.08)
                    } else {
                        (voice.age - 0.10, 0.18)
                    };
                    (age / 0.0015).min(1.0) * (1.0 - age / duration).max(0.0).powf(1.3)
                } else if voice.kind == DEATH {
                    (voice.age / 0.025).min(1.0) * (1.0 - t).powf(1.3)
                } else {
                    attack * (1.0 - t).powi(3)
                };
                voice.filter += (noise - voice.filter)
                    * match voice.kind {
                        FOOT => 0.025,
                        ENTITY => 0.02,
                        _ => 0.08,
                    };
                let frequency = match voice.kind {
                    FOOT => 115.0 - 18.0 * t,
                    ENTITY => 94.0 - 12.0 * t,
                    METAL => 317.0,
                    RAGE => 92.0 + 120.0 * t,
                    SWING => 180.0 - 110.0 * t,
                    HIT => 72.0 - 30.0 * t,
                    DEATH => 85.0 - 63.0 * t,
                    EXIT => 146.83,
                    _ => 900.0,
                };
                voice.phase = (voice.phase + frequency / RATE).fract();
                let tone = wave(&self.table, voice.phase);
                let sound = match voice.kind {
                    FOOT => {
                        tone * 1.25
                            + wave(&self.table, voice.age * 205.0) * 0.30
                            + voice.filter * 0.04
                    }
                    ENTITY => {
                        tone * 1.45
                            + wave(&self.table, voice.age * 273.0) * 0.50
                            + wave(&self.table, voice.age * 419.0) * 0.18
                            + voice.filter * 0.05
                    }
                    HIT => {
                        tone * 1.3
                            + wave(&self.table, voice.age * 168.0) * 0.45
                            + voice.filter * 0.12
                    }
                    SWING => {
                        tone * 0.65
                            + wave(&self.table, voice.phase * 2.73) * 0.4
                            + voice.filter * 0.08
                    }
                    SWITCH => tone * 0.65 + voice.filter * 0.12,
                    COIN => {
                        // Original high-register, two-note arcade pickup. The
                        // pitch steps upward; it is not a continuous sweep.
                        let (age, frequency) = if voice.age < 0.10 {
                            (voice.age, 1046.5)
                        } else {
                            (voice.age - 0.10, 1396.91)
                        };
                        let phase = age * frequency;
                        let pulse = wave(&self.table, phase) * 0.78
                            + wave(&self.table, phase * 3.0) * 0.14
                            + wave(&self.table, phase * 5.0) * 0.045
                            + wave(&self.table, phase * 7.0) * 0.015;
                        // Five-bit amplitude steps give the pulse a restrained
                        // pixel-era texture; the shared envelope stays smooth.
                        (pulse * 16.0).round().clamp(-16.0, 15.0) / 16.0
                    }
                    METAL => {
                        (wave(&self.table, voice.age * 317.0)
                            + wave(&self.table, voice.age * 503.7) * 0.55
                            + wave(&self.table, voice.age * 811.1) * 0.3)
                            * 0.32
                    }
                    DEATH => {
                        // A descending, dissonant chord in the audible midrange,
                        // with a low impact underneath; no scream or recording.
                        let phase = 246.94 * voice.age - 20.0 * voice.age * voice.age;
                        wave(&self.table, phase) * 0.75
                            + wave(&self.table, phase * 1.1892) * 0.55
                            + wave(&self.table, phase * core::f32::consts::SQRT_2) * 0.35
                            + wave(&self.table, voice.age * 98.0) * 0.35 * (1.0 - t).powi(3)
                    }
                    RAGE => {
                        wave(
                            &self.table,
                            voice.phase + 0.18 * wave(&self.table, voice.age * 37.1),
                        ) * 0.85
                            + voice.filter * 0.07
                    }
                    EXIT => {
                        (tone
                            + wave(&self.table, voice.age * 220.0) * 0.5
                            + wave(&self.table, voice.age * 293.66) * 0.3)
                            * 0.60
                    }
                    _ => voice.filter * 0.75 + tone * 0.2,
                } * envelope
                    * voice.gain;
                let left = sound * (0.5 - voice.pan * 0.4);
                let right = sound * (0.5 + voice.pan * 0.4);
                stereo[0] += left;
                stereo[1] += right;
                let wet = match voice.kind {
                    FOOT => 0.10,
                    ENTITY => 0.35,
                    COIN => 0.0,
                    _ => 0.65,
                };
                send[0] += left * wet;
                send[1] += right * wet;
                voice.age += 1.0 / RATE;
            }
            let echo = self.delay[self.cursor];
            self.delay[self.cursor] = [send[0] + echo[1] * 0.32, send[1] + echo[0] * 0.32];
            self.cursor = (self.cursor + 1) % self.delay.len();
            for ch in 0..2 {
                let dry = (stereo[ch] + echo[ch] * 0.22) * self.gain;
                let filtered = dry - self.dc_in[ch] + 0.995 * self.dc_out[ch];
                self.dc_in[ch] = dry;
                self.dc_out[ch] = filtered;
                frame[ch] = filtered / (1.0 + filtered.abs());
            }
        }
    }
}
fn wave(table: &[f32; 2048], phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0) * 2048.0;
    let i = p as u32 as usize & 2047;
    table[i] + (table[(i + 1) & 2047] - table[i]) * (p - p.floor())
}
unsafe extern "C" {
    fn halls_audio_open(
        callback: unsafe extern "C" fn(*mut c_void, *mut f32, u32),
        user: *mut c_void,
    ) -> *mut c_void;
    fn halls_audio_close(device: *mut c_void);
    fn halls_audio_running(device: *mut c_void) -> i32;
}
unsafe extern "C" fn callback(user: *mut c_void, output: *mut f32, frames: u32) {
    // The native backend serializes callbacks; stereo f32 output has exactly frames * 2 samples.
    // The stable Box lives until the backend has stopped and joined its audio worker.
    unsafe {
        (&mut *user.cast::<Synth>())
            .render(core::slice::from_raw_parts_mut(output, frames as usize * 2));
    }
}
#[derive(Default)]
struct EntitySound {
    travel: f32,
    attack: f32,
    raging: bool,
}
pub struct Audio {
    device: *mut c_void,
    synth: *mut Synth,
    controls: Arc<Controls>,
    muted: bool,
    paused: bool,
    volume: f32,
    distance: f32,
    foot: f32,
    health: f32,
    lamp: bool,
    ended: bool,
    coins: u8,
    entities: Vec<EntitySound>,
}
impl Audio {
    pub fn new(muted: bool) -> Self {
        let controls = Arc::new(Controls::new());
        let synth = Box::into_raw(Box::new(Synth::new(controls.clone(), 0x48414c4c53)));
        let mut audio = Self {
            device: core::ptr::null_mut(),
            synth,
            controls,
            muted,
            paused: false,
            volume: 0.55,
            distance: 0.0,
            foot: 0.0,
            health: 100.0,
            lamp: true,
            ended: false,
            coins: 0,
            entities: Vec::new(),
        };
        if !muted {
            audio.open();
        }
        audio.apply_gain();
        audio
    }
    fn open(&mut self) {
        // The callback owns mutable access to the Synth until close returns.
        unsafe {
            halls_audio_close(self.device);
            self.controls.ending.store(0, Ordering::Release);
            self.controls.reset.fetch_add(1, Ordering::Release);
            self.device = halls_audio_open(callback, self.synth.cast());
        }
    }
    pub fn available(&self) -> bool {
        unsafe { halls_audio_running(self.device) != 0 }
    }
    pub fn toggle(&mut self) {
        if !self.muted && !self.available() {
            self.open();
        } else {
            self.muted = !self.muted;
            if !self.muted && !self.available() {
                self.open();
            }
        }
        self.apply_gain();
    }
    pub fn volume(&mut self, delta: f32) {
        self.volume = (self.volume + delta).clamp(0.0, 1.0);
        self.apply_gain();
    }
    pub fn pause(&mut self, paused: bool) {
        self.paused = paused;
        self.apply_gain();
    }
    fn apply_gain(&self) {
        set(
            &self.controls.gain,
            if self.muted || self.paused {
                0.0
            } else {
                self.volume
            },
        );
    }
    pub fn status(&self) -> String {
        if self.muted {
            "M muted".into()
        } else if !self.available() {
            "M retry audio".into()
        } else {
            let mut text = crate::console::Buffer::default();
            text.bytes(b"M sound ")
                .number((self.volume * 100.0).round() as usize)
                .bytes(b"%");
            text.into_text()
        }
    }
    fn event(&self, kind: u8, gain: f32, pan: f32) {
        if !self.muted && !self.paused && gain > 0.001 {
            if matches!(kind, DEATH | EXIT) {
                self.controls.ending.store(kind as u32, Ordering::Release);
            } else {
                self.controls.push(kind, gain, pan);
            }
        }
    }
    pub fn reset(&mut self, game: &Game, player: &Player) {
        self.distance = player.distance;
        self.foot = 0.0;
        self.health = game.progress.health;
        self.lamp = game.flashlight;
        self.ended = false;
        self.coins = game.coins.collected_mask;
        self.controls.ending.store(0, Ordering::Release);
        self.entities = game
            .progress
            .entities
            .iter()
            .map(|e| EntitySound {
                travel: e.travel,
                attack: e.attack_left,
                raging: e.hate >= 100.0,
            })
            .collect();
        set(&self.controls.threat, 0.0);
        set(&self.controls.ambience, 1.0);
        self.controls.reset.fetch_add(1, Ordering::Release);
    }
    pub fn sync(&mut self, map: &Map, game: &Game, player: &Player) {
        let picked = (game.coins.collected_mask & !self.coins).count_ones();
        // Consume pickups even while muted, paused or at zero volume. A redraw
        // or later unmute must not replay a previously collected coin.
        self.coins = game.coins.collected_mask;
        if picked != 0 && self.volume > 0.0 && !game.dead() {
            self.event(COIN, (0.58 + picked as f32 * 0.06).min(0.76), 0.0);
        }
        let hurt = game.progress.health < self.health;
        if game.dead() && !self.ended {
            self.event(DEATH, 1.0, 0.0);
        } else if hurt && !game.dead() {
            self.event(HIT, 0.85, 0.0);
        }
        if player.won && !self.ended {
            self.event(EXIT, 0.65, 0.0);
        }
        if game.flashlight != self.lamp {
            self.event(SWITCH, 0.3, 0.0);
        }
        if !hurt && !game.dead() && !player.won && game.knocked <= 0.0 {
            self.foot += (player.distance - self.distance).max(0.0);
            let stride = if game.running { 1.55 } else { 1.35 };
            if self.foot >= stride {
                self.foot %= stride;
                self.event(FOOT, if game.running { 0.72 } else { 0.52 }, 0.0);
            }
        }
        self.distance = player.distance;
        self.health = game.progress.health;
        self.lamp = game.flashlight;
        self.ended = game.dead() || player.won;
        let mut threat = 0.0f32;
        let mut pan = 0.0;
        for (i, entity) in game.progress.entities.iter().enumerate() {
            let p = Vec3::new(
                entity.position[0],
                player.camera.position.y,
                entity.position[1],
            );
            let delta = p - player.camera.position;
            let distance = delta.length();
            let direction = delta.dot(player.camera.right()) / distance.max(0.1);
            let audible = (1.0 - distance / 48.0).clamp(0.0, 1.0)
                / (1.0 + distance * distance * 0.005)
                * if map.light_visible(player.camera.position, p) {
                    1.0
                } else {
                    0.55
                };
            let pressure = (entity.hate / 100.0) * (1.0 - distance / 60.0).clamp(0.0, 1.0).sqrt();
            if pressure > threat {
                threat = pressure;
                pan = direction;
            }
            if i >= self.entities.len() {
                self.entities.push(EntitySound::default());
            }
            let state = &self.entities[i];
            if !self.ended {
                if entity.travel - state.travel >= 0.68 {
                    self.event(ENTITY, 1.0 * audible, direction);
                }
                if entity.attack_left > state.attack + 0.1 {
                    self.event(SWING, 0.95 * audible, direction);
                }
                if entity.hate >= 100.0 && !state.raging {
                    self.event(RAGE, 1.0 * audible, direction);
                }
            }
            let state = &mut self.entities[i];
            if entity.travel - state.travel >= 0.68 {
                state.travel = entity.travel;
            }
            state.attack = entity.attack_left;
            if entity.hate >= 100.0 {
                state.raging = true;
            } else if entity.hate < 70.0 {
                state.raging = false;
            }
        }
        set(&self.controls.threat, if self.ended { 0.0 } else { threat });
        set(&self.controls.pan, pan);
        set(&self.controls.ambience, if self.ended { 0.0 } else { 1.0 });
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        set(&self.controls.gain, 0.0);
        if !self.device.is_null() {
            crate::runtime::sleep(core::time::Duration::from_millis(40));
        }
        // Stop and join the callback before reclaiming its Box. Never accessed elsewhere.
        unsafe {
            halls_audio_close(self.device);
            drop(Box::from_raw(self.synth));
        }
    }
}

#[cfg(feature = "tools")]
pub fn demo(seconds: u32) -> bool {
    let audio = Audio::new(false);
    if !audio.available() {
        return false;
    }
    for tick in 0..seconds.min(120) * 10 {
        let t = tick as f32 / 10.0;
        set(
            &audio.controls.threat,
            if t > 8.0 && t < 16.0 { 0.9 } else { 0.0 },
        );
        set(&audio.controls.pan, (t * 0.6).sin());
        if tick % 6 == 0 && t > 3.0 && t < 16.0 {
            audio.event(FOOT, 0.52, 0.0);
        }
        match tick {
            20 => audio.event(METAL, 0.15, -0.8),
            60 => audio.event(ENTITY, 1.0, 0.8),
            90 => audio.event(RAGE, 0.55, 0.6),
            130 => audio.event(HIT, 0.7, -0.4),
            170 => audio.event(EXIT, 0.65, 0.0),
            _ => {}
        }
        crate::runtime::sleep(core::time::Duration::from_millis(100));
    }
    audio.available()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_bounded_and_stereo() {
        let controls = Arc::new(Controls::new());
        set(&controls.gain, 0.55);
        let mut a = Synth::new(controls.clone(), 42);
        let mut b = Synth::new(controls, 42);
        for kind in FOOT..=SWITCH {
            a.trigger(kind, 1.0, 0.8);
            b.trigger(kind, 1.0, 0.8);
        }
        let mut left = vec![0.0; 48000 * 10];
        let mut right = left.clone();
        a.render(&mut left);
        b.render(&mut right);
        assert_eq!(left, right);
        assert!(left.iter().all(|v| v.is_finite() && v.abs() < 0.85));
        assert!(
            left.as_chunks::<2>()
                .0
                .iter()
                .any(|f| (f[0] - f[1]).abs() > 0.01)
        );
        let mean = left.iter().sum::<f32>() / left.len() as f32;
        assert!(mean.abs() < 0.001);
        assert!(left.windows(2).step_by(2).any(|v| v[0] != 0.0));
        assert!(a.voices.iter().all(|v| v.age >= v.duration));
    }
    #[test]
    fn mute_fades_to_silence_and_reset_clears_voices() {
        let controls = Arc::new(Controls::new());
        set(&controls.gain, 0.55);
        let mut synth = Synth::new(controls.clone(), 8);
        synth.trigger(DEATH, 1.0, 0.0);
        let mut frame = vec![0.0; 48000];
        synth.render(&mut frame);
        set(&controls.gain, 0.0);
        synth.render(&mut frame);
        assert!(frame[frame.len() - 1000..].iter().all(|s| s.abs() < 1e-7));
        controls.reset.fetch_add(1, Ordering::Release);
        synth.render(&mut frame);
        assert!(synth.voices.iter().all(|v| v.duration == 0.0));
    }
    #[test]
    fn queue_preserves_order_and_drops_overflow() {
        let controls = Controls::new();
        for i in 1..100 {
            controls.push(i, 0.5, -0.5);
        }
        for i in 1..QUEUE {
            let (kind, gain, pan) = controls.pop().unwrap();
            assert_eq!(kind, i as u8);
            assert!((gain - 0.5).abs() < 0.001 && (pan + 0.5).abs() < 0.001);
        }
        assert!(controls.pop().is_none());
        controls.push(EXIT, 1.0, 1.0);
        assert_eq!(controls.pop().unwrap().0, EXIT);
    }
    #[test]
    fn movement_damage_and_end_events_follow_actual_game_state() {
        let map = Map::generated(17, 42).unwrap();
        let mut game = Game::new(&map);
        game.progress.entities.clear();
        let mut player = Player::new(&map);
        // Leave the device unopened; inspect the same queue the callback consumes.
        let mut audio = Audio::new(true);
        audio.muted = false;
        audio.reset(&game, &player);
        audio.sync(&map, &game, &player);
        assert!(audio.controls.pop().is_none());
        player.distance += 1.36;
        audio.sync(&map, &game, &player);
        assert_eq!(audio.controls.pop().unwrap().0, FOOT);
        audio.sync(&map, &game, &player);
        assert!(audio.controls.pop().is_none());
        player.distance += 0.85; // The existing hit response moves the camera.
        game.progress.health -= 34.0;
        audio.sync(&map, &game, &player);
        assert_eq!(audio.controls.pop().unwrap().0, HIT);
        assert!(
            audio.controls.pop().is_none(),
            "knockback is not a footstep"
        );
        game.progress.health = 0.0;
        audio.sync(&map, &game, &player);
        assert_eq!(
            audio.controls.ending.swap(0, Ordering::AcqRel),
            DEATH as u32
        );
        audio.sync(&map, &game, &player);
        assert!(audio.controls.pop().is_none(), "death plays once");
        game.progress.health = 100.0;
        audio.reset(&game, &player);
        player.won = true;
        audio.sync(&map, &game, &player);
        assert_eq!(audio.controls.ending.swap(0, Ordering::AcqRel), EXIT as u32);
        audio.sync(&map, &game, &player);
        assert!(audio.controls.pop().is_none(), "exit plays once");
        audio.pause(true);
        game.flashlight = !game.flashlight;
        audio.sync(&map, &game, &player);
        assert!(audio.controls.pop().is_none(), "pause suppresses new cues");
    }
    #[test]
    fn coin_pickups_play_once_and_obey_mute_pause_and_volume() {
        let map = Map::generated(17, 42).unwrap();
        let mut game = Game::new(&map);
        game.progress.entities.clear();
        let player = Player::new(&map);
        let mut audio = Audio::new(true);
        audio.muted = false;
        audio.reset(&game, &player);
        game.coins.collected_mask = 1;
        audio.sync(&map, &game, &player);
        assert_eq!(audio.controls.pop().unwrap().0, COIN);
        audio.sync(&map, &game, &player);
        assert!(audio.controls.pop().is_none());
        game.coins.collected_mask = 7;
        audio.sync(&map, &game, &player);
        assert_eq!(audio.controls.pop().unwrap().0, COIN);
        assert!(
            audio.controls.pop().is_none(),
            "simultaneous pickups form one cue"
        );
        for mode in 0..3 {
            game.coins.collected_mask = 0;
            audio.reset(&game, &player);
            audio.muted = mode == 0;
            audio.paused = mode == 1;
            audio.volume = if mode == 2 { 0.0 } else { 0.55 };
            game.coins.collected_mask = 1;
            audio.sync(&map, &game, &player);
            assert!(audio.controls.pop().is_none());
            audio.muted = false;
            audio.paused = false;
            audio.volume = 0.55;
            audio.sync(&map, &game, &player);
            assert!(
                audio.controls.pop().is_none(),
                "unmute does not replay a pickup"
            );
        }
        audio.reset(&game, &player);
        audio.sync(&map, &game, &player);
        assert!(
            audio.controls.pop().is_none(),
            "reset does not replay existing coins"
        );
    }
    #[test]
    fn coin_tone_is_short_bounded_and_uses_the_shared_output_gain() {
        let controls = Arc::new(Controls::new());
        set(&controls.gain, 0.55);
        set(&controls.ambience, 0.0);
        let mut synth = Synth::new(controls.clone(), 42);
        synth.ambience = 0.0;
        synth.trigger(COIN, 0.64, 0.0);
        let mut pcm = vec![0.0; RATE as usize * 2];
        synth.render(&mut pcm);
        assert!(pcm.iter().all(|s| s.is_finite() && s.abs() < 0.4));
        assert!(pcm.iter().any(|s| s.abs() > 0.02));
        let energy = |from: f32, to: f32| {
            let range = &pcm[(from * RATE) as usize * 2..(to * RATE) as usize * 2];
            range.iter().map(|s| s * s).sum::<f32>() / range.len() as f32
        };
        let first = energy(0.01, 0.06);
        let second = energy(0.115, 0.20);
        assert!(first > 0.0001 && second > 0.0001, "both notes are audible");
        assert!(
            energy(0.087, 0.098) < first * 0.01,
            "the two attacks have a clear gap"
        );
        assert!(
            energy(0.35, 0.60) < first * 0.001,
            "there is no delayed third note"
        );
        assert!(pcm[pcm.len() - 4800..].iter().all(|s| s.abs() < 0.005));
        set(&controls.gain, 0.0);
        synth.render(&mut pcm);
        synth.trigger(COIN, 0.64, 0.0);
        synth.render(&mut pcm);
        assert!(pcm.iter().all(|s| *s == 0.0));
    }
    #[test]
    fn sixty_second_mix_has_headroom_and_continuous_output() {
        let controls = Arc::new(Controls::new());
        set(&controls.gain, 0.55);
        let mut synth = Synth::new(controls.clone(), 93);
        let mut block = [0.0; 960];
        let mut peak = 0.0f32;
        let mut jump = 0.0f32;
        let mut energy = 0.0f64;
        let mut previous = [0.0f32; 2];
        let start = std::time::Instant::now();
        for tick in 0..6000 {
            if tick % 60 == 0 {
                controls.push(FOOT, 0.55, 0.0);
            }
            if tick % 97 == 0 {
                controls.push(ENTITY, 0.55, -0.7);
            }
            if tick % 700 == 0 {
                controls.push(RAGE, 0.7, 0.8);
            }
            if tick % 500 == 0 {
                controls.push(HIT, 0.85, 0.0);
            }
            set(&controls.threat, if tick % 2000 < 1000 { 1.0 } else { 0.0 });
            synth.render(&mut block);
            for frame in block.as_chunks::<2>().0 {
                for ch in 0..2 {
                    assert!(frame[ch].is_finite());
                    peak = peak.max(frame[ch].abs());
                    jump = jump.max((frame[ch] - previous[ch]).abs());
                    energy += (frame[ch] as f64).powi(2);
                    previous[ch] = frame[ch];
                }
            }
        }
        let rms = (energy / (6000.0 * 960.0)).sqrt();
        println!(
            "60s synthesized in {:.3}s; peak={peak:.5}, RMS={rms:.5}, max adjacent delta={jump:.5}",
            start.elapsed().as_secs_f64()
        );
        assert!(peak < 0.8 && rms > 0.005 && rms < 0.2 && jump < 0.15);
    }
    #[test]
    fn human_step_cadence_at_game_speeds() {
        let map = Map::generated(17, 42).unwrap();
        for (running, speed, expected) in [(false, 2.7, 20), (true, 4.6, 29)] {
            let mut game = Game::new(&map);
            game.progress.entities.clear();
            game.running = running;
            let mut player = Player::new(&map);
            let mut audio = Audio::new(true);
            audio.muted = false;
            audio.reset(&game, &player);
            let mut count = 0;
            for _ in 0..300 {
                player.distance += speed / 30.0;
                audio.sync(&map, &game, &player);
                while let Some((kind, _, _)) = audio.controls.pop() {
                    if kind == FOOT {
                        count += 1;
                    }
                }
            }
            assert!(
                (expected - 1..=expected + 1).contains(&count),
                "{count} footsteps in 10s"
            );
        }
    }
    #[test]
    fn failure_cue_survives_reset_and_has_audible_midrange_energy() {
        let controls = Arc::new(Controls::new());
        set(&controls.gain, 0.55);
        set(&controls.ambience, 0.0);
        let mut synth = Synth::new(controls.clone(), 2);
        controls.reset.fetch_add(1, Ordering::Release);
        controls.ending.store(DEATH as u32, Ordering::Release);
        let mut samples = vec![0.0; 48000 * 2 * 3];
        synth.render(&mut samples);
        let energy = |from: usize, to: usize| {
            let slice = &samples[from * 2..to * 2];
            (slice.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / slice.len() as f64).sqrt()
        };
        assert!(energy(0, 48000) > 0.05, "failure too quiet");
        assert!(
            energy(48000, 96000) > 0.01,
            "failure decays before it is recognizable"
        );
        assert_eq!(controls.ending.load(Ordering::Acquire), 0);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() < 0.9));
    }
    #[test]
    fn a_scene_that_starts_dead_emits_failure_even_without_a_health_delta() {
        let map = Map::generated(17, 1).unwrap();
        let mut game = Game::new(&map);
        game.progress.health = 0.0;
        let player = Player::new(&map);
        let mut audio = Audio::new(true);
        audio.muted = false;
        audio.reset(&game, &player);
        audio.sync(&map, &game, &player);
        assert_eq!(
            audio.controls.ending.swap(0, Ordering::AcqRel),
            DEATH as u32
        );
        audio.sync(&map, &game, &player);
        assert_eq!(audio.controls.ending.load(Ordering::Acquire), 0);
    }
}
