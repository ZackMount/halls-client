//! Small OS-backed services shared by the no_std game and its test harness.
pub use core::time::Duration;
use core::{
    alloc::{GlobalAlloc, Layout},
    ops::{Add, Sub},
};
unsafe extern "C" {
    fn halls_alloc(size: usize, align: usize) -> *mut u8;
    fn halls_free(pointer: *mut u8);
    fn halls_clock() -> u64;
    fn halls_sleep(ns: u64);
    fn halls_exit(code: i32) -> !;
    fn halls_executable_name(out: *mut u8, capacity: u32) -> u32;

}
#[cfg(target_arch = "x86_64")]
use crate::math_x87::{
    atan2 as atan2f, ceil as ceilf, cos as cosf, exp as expf, floor as floorf, hypot as hypotf,
    pow as powf, round as roundf, sin as sinf, sqrt as sqrtf, tan as tanf, trunc as truncf,
    trunc64 as trunc,
};
#[cfg(not(target_arch = "x86_64"))]
unsafe extern "C" {
    fn tanf(x: f32) -> f32;
    fn sinf(x: f32) -> f32;
    fn cosf(x: f32) -> f32;
    fn sqrtf(x: f32) -> f32;
    fn floorf(x: f32) -> f32;
    fn ceilf(x: f32) -> f32;
    fn roundf(x: f32) -> f32;
    fn truncf(x: f32) -> f32;
    fn expf(x: f32) -> f32;
    fn powf(x: f32, y: f32) -> f32;
    fn atan2f(x: f32, y: f32) -> f32;
    fn hypotf(x: f32, y: f32) -> f32;
    fn trunc(x: f64) -> f64;
}
pub struct Allocator;
// The native adapter honors Layout alignment, and frees through the same allocator.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { halls_alloc(layout.size(), layout.align()) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        unsafe { halls_free(pointer) }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        Self(unsafe { halls_clock() })
    }
    pub fn elapsed(self) -> Duration {
        Self::now() - self
    }
}
impl Add<Duration> for Instant {
    type Output = Self;
    fn add(self, d: Duration) -> Self {
        Self(self.0.saturating_add(d.as_nanos() as u64))
    }
}
impl Sub for Instant {
    type Output = Duration;
    fn sub(self, b: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(b.0))
    }
}
pub fn sleep(d: Duration) {
    unsafe { halls_sleep(d.as_nanos().min(u64::MAX as u128) as u64) }
}
static PROTOCOL: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
pub(crate) fn protocol(active: bool) {
    PROTOCOL.store(active, core::sync::atomic::Ordering::Relaxed);
}
pub fn exit(code: i32) -> ! {
    if PROTOCOL.swap(false, core::sync::atomic::Ordering::Relaxed) {
        unsafe extern "C" {
            fn halls_write(channel: i32, bytes: *const u8, length: usize) -> i32;
        }
        unsafe {
            halls_write(1, b"\x1b[<u".as_ptr(), 4);
        }
    }
    unsafe { halls_exit(code) }
}
// Core provides IEEE arithmetic but not the hosted libm methods. Keep the same
// system math functions, rather than changing the visuals with approximations.
pub trait Float: Sized {
    fn sin_cos(self) -> (Self, Self);
    fn tan(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn sqrt(self) -> Self;
    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn fract(self) -> Self;
    fn exp(self) -> Self;
    fn powf(self, b: Self) -> Self;
    fn atan2(self, b: Self) -> Self;
    fn hypot(self, b: Self) -> Self;
    fn powi(self, n: i32) -> Self;
    fn rem_euclid(self, b: Self) -> Self;
}
#[allow(unused_unsafe)]
impl Float for f32 {
    fn sin_cos(self) -> (Self, Self) {
        #[cfg(target_arch = "x86_64")]
        {
            crate::math_x87::sin_cos(self)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            unsafe { (sinf(self), cosf(self)) }
        }
    }
    fn tan(self) -> Self {
        unsafe { tanf(self) }
    }
    fn sin(self) -> Self {
        unsafe { sinf(self) }
    }
    fn cos(self) -> Self {
        unsafe { cosf(self) }
    }
    fn sqrt(self) -> Self {
        unsafe { sqrtf(self) }
    }
    fn floor(self) -> Self {
        unsafe { floorf(self) }
    }
    fn ceil(self) -> Self {
        unsafe { ceilf(self) }
    }
    fn round(self) -> Self {
        unsafe { roundf(self) }
    }
    fn fract(self) -> Self {
        self - unsafe { truncf(self) }
    }
    fn exp(self) -> Self {
        unsafe { expf(self) }
    }
    fn powf(self, b: Self) -> Self {
        unsafe { powf(self, b) }
    }
    fn atan2(self, b: Self) -> Self {
        unsafe { atan2f(self, b) }
    }
    fn hypot(self, b: Self) -> Self {
        unsafe { hypotf(self, b) }
    }
    fn powi(self, n: i32) -> Self {
        let mut base = self;
        let mut exponent = n.unsigned_abs();
        let mut result = 1.0;
        while exponent > 0 {
            if exponent & 1 != 0 {
                result *= base;
            }
            exponent >>= 1;
            if exponent > 0 {
                base *= base;
            }
        }
        if n < 0 { 1.0 / result } else { result }
    }
    fn rem_euclid(self, b: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        let r = crate::math_x87::fmodf(self, b);
        #[cfg(not(target_arch = "x86_64"))]
        let r = self % b;
        if r < 0.0 { r + b.abs() } else { r }
    }
}
#[allow(unused_unsafe)]
pub fn fractional_time(x: f64) -> f32 {
    (x - unsafe { trunc(x) }) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_allocator_honors_large_alignments() {
        for alignment in [1, 8, 16, 64, 4096] {
            let layout = Layout::from_size_align(8193, alignment).unwrap();
            unsafe {
                let p = Allocator.alloc(layout);
                assert!(!p.is_null());
                assert_eq!(p as usize % alignment, 0);
                p.write(17);
                p.add(8192).write(29);
                Allocator.dealloc(p, layout);
            }
        }
    }
    #[test]
    fn native_clock_is_monotonic_and_sleep_advances_it() {
        let a = Instant::now();
        sleep(Duration::from_millis(2));
        assert!(a.elapsed() >= Duration::from_millis(1));
    }
    #[test]
    fn core_math_matches_hosted_math_for_game_inputs() {
        for x in [-2.5_f32, -0.1, 0., 0.25, 0.7, 1.0, 3.5] {
            assert_eq!(Float::sin(x).to_bits(), x.sin().to_bits());
            assert_eq!(Float::floor(x).to_bits(), x.floor().to_bits());
            assert_eq!(Float::fract(x).to_bits(), x.fract().to_bits());
            for n in [2, 3, 6, 20] {
                assert!(
                    (Float::powi(x, n) - x.powi(n)).abs() <= x.powi(n).abs() * 0.000001 + 0.000001
                );
            }
        }
    }
}

/// A display-safe basename of the running image. Empty means the OS lookup failed.
pub fn executable_name() -> alloc::string::String {
    let mut bytes = [0u8; 1024];
    let length = unsafe { halls_executable_name(bytes.as_mut_ptr(), bytes.len() as u32) } as usize;
    let name = alloc::string::String::from_utf8_lossy(bytes.get(..length).unwrap_or_default());
    name.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod executable_name_tests {
    #[test]
    fn reports_the_actual_test_executable_instead_of_a_fixed_game_name() {
        let path = std::env::current_exe().unwrap();
        let expected = path.file_name().unwrap().to_string_lossy();
        assert_eq!(super::executable_name(), expected);
    }
}
