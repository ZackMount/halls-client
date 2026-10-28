#![no_std]
#![no_main]
#[path = "../../../src/math_x87.rs"]
mod math;
core::arch::global_asm!(include_str!(concat!(env!("OUT_DIR"), "/memory.s")));
unsafe extern "win64" {
    fn probe_memcpy(to: *mut u8, from: *const u8, n: usize) -> *mut u8;
    fn probe_memmove(to: *mut u8, from: *const u8, n: usize) -> *mut u8;
    fn probe_memset(to: *mut u8, value: i32, n: usize) -> *mut u8;
    fn probe_memcmp(a: *const u8, b: *const u8, n: usize) -> i32;
    fn probe___chkstk();
}
fn memory_tests() {
    let source = core::array::from_fn::<_, 512, _>(|i| (i * 37) as u8);
    for n in 0..=256 {
        let mut out = [0xa5u8; 512];
        let p = unsafe { out.as_mut_ptr().add(17) };
        unsafe {
            assert_eq!(probe_memcpy(p, source.as_ptr(), n), p);
            assert_eq!(probe_memcmp(p, source.as_ptr(), n), 0);
        }
        assert_eq!(&out[17..17 + n], &source[..n]);
        assert!(out[..17].iter().all(|&x| x == 0xa5));
        assert!(out[17 + n..].iter().all(|&x| x == 0xa5));
        unsafe {
            assert_eq!(probe_memset(p, 0x18b, n), p);
        }
        assert!(out[17..17 + n].iter().all(|&x| x == 0x8b));
        for (from, to) in [(0, 0), (0, 1), (1, 0), (0, 16), (16, 0), (0, 256)] {
            let mut actual = source;
            let mut expected = source;
            expected.copy_within(from..from + n, to);
            unsafe {
                let p = actual.as_mut_ptr().add(to);
                assert_eq!(probe_memmove(p, actual.as_ptr().add(from), n), p);
            }
            assert_eq!(actual, expected);
        }
    }
    for i in 0..255 {
        let a = [i as u8];
        let b = [(i + 1) as u8];
        unsafe {
            assert!(probe_memcmp(a.as_ptr(), b.as_ptr(), 1) < 0);
            assert!(probe_memcmp(b.as_ptr(), a.as_ptr(), 1) > 0);
        }
    }
    for n in [0usize, 8, 4095, 4096, 4097, 8192, 32768] {
        let mut size = n;
        let mut marker = 0x12345678usize;
        unsafe {
            core::arch::asm!("sub rsp,32","call {probe}","add rsp,32",probe=sym probe___chkstk,inout("rax")size,inout("rcx")marker,clobber_abi("win64"));
        }
        assert_eq!(size, n);
        assert_eq!(marker, 0x12345678);
    }
}

unsafe extern "C" {
    fn exit(code: i32) -> !;
    fn printf(format: *const u8, ...) -> i32;
    fn sinf(x: f32) -> f32;
    fn cosf(x: f32) -> f32;
    fn tanf(x: f32) -> f32;
    fn sqrtf(x: f32) -> f32;
    fn floorf(x: f32) -> f32;
    fn ceilf(x: f32) -> f32;
    fn roundf(x: f32) -> f32;
    fn truncf(x: f32) -> f32;
    fn expf(x: f32) -> f32;
    fn powf(x: f32, y: f32) -> f32;
    fn atan2f(y: f32, x: f32) -> f32;
    fn hypotf(x: f32, y: f32) -> f32;
    fn fmodf(x: f32, y: f32) -> f32;
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { exit(99) }
}
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() -> ! {
    unsafe { exit(99) }
}
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    memory_tests();
    let mut maximum = 0.0f32;
    let mut n = 0usize;
    for i in -10000..=10000 {
        let x = i as f32 / 64.0;
        let (sin, cos) = math::sin_cos(x);
        let pairs = unsafe {
            [
                (sin, sinf(x)),
                (cos, cosf(x)),
                (math::sin(x), sinf(x)),
                (math::cos(x), cosf(x)),
                (math::sqrt(x.abs()), sqrtf(x.abs())),
                (math::floor(x), floorf(x)),
                (math::ceil(x), ceilf(x)),
                (math::round(x), roundf(x)),
                (math::trunc(x), truncf(x)),
                (math::exp(-x.abs() / 8.0), expf(-x.abs() / 8.0)),
                (math::pow(x.abs(), 0.55), powf(x.abs(), 0.55)),
                (math::pow(x.abs(), 1.0 / 2.4), powf(x.abs(), 1.0 / 2.4)),
                (math::atan2(x, 0.31), atan2f(x, 0.31)),
                (math::hypot(x, 2.71), hypotf(x, 2.71)),
                (math::fmodf(x, 0.6), fmodf(x, 0.6)),
            ]
        };
        for (got, want) in pairs {
            let e = (got - want).abs() / (1.0 + want.abs());
            maximum = maximum.max(e);
            n += 1;
            if !got.is_finite() || e > 0.000002 {
                unsafe {
                    printf(
                        c"FAIL x=%f got=%f want=%f\n".as_ptr().cast(),
                        x as f64,
                        got as f64,
                        want as f64,
                    );
                }
                return 1;
            }
        }
    }
    for x in [-1.0_f32, -0.75, 0., 0.7, 1.] {
        assert!((math::tan(x) - unsafe { tanf(x) }).abs() < 0.000001);
    }
    assert_eq!(math::trunc64(12345.6789), 12345.0);
    assert_eq!(math::pow(0., 0.), 1.);
    assert_eq!(math::pow(0., 1.3), 0.);
    assert!(math::fmodf(f32::INFINITY, 2.).is_nan());
    unsafe {
        printf(
            c"PASS Win64 memory/stack ABI and %llu scalar comparisons; max normalized error %.9g\n"
                .as_ptr()
                .cast(),
            n as u64,
            maximum as f64,
        );
    }
    0
}
