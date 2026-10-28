//! Organizer-only native provider boundary tests; never linked into the game.
use std::{ffi::c_void, path::PathBuf};
unsafe extern "C" {
    fn halls_digest_open() -> *mut c_void;
    fn halls_digest_update(state: *mut c_void, data: *const u8, size: u32) -> i32;
    fn halls_digest_finish(state: *mut c_void, out: *mut u8) -> i32;
    fn halls_digest_close(state: *mut c_void);
    fn halls_envelope_open(size: u32, plain: u32, out: *mut u8) -> *mut c_void;
    fn halls_envelope_update(state: *mut c_void, data: *const u8, size: u32, out: *mut u8) -> i32;
    fn halls_envelope_finish(state: *mut c_void) -> i32;
    fn halls_envelope_close(state: *mut c_void);
}
fn main() {
    // Retain the native archive even when this example only uses its C ABI.
    std::hint::black_box(halls::player::RUN_SPEED);
    let folder = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&folder).unwrap();
    for size in [1usize, 15, 16, 17, 4095, 4096, 4097, 131101, 2097161] {
        let raw: Vec<u8> = (0..size)
            .map(|i| ((i * 73) ^ (i / 13) ^ (i * i / 193)) as u8)
            .collect();
        let mut signature = [0u8; 64];
        unsafe {
            let digest = halls_digest_open();
            assert!(!digest.is_null());
            for chunk in raw.chunks(4096) {
                assert_eq!(
                    halls_digest_update(digest, chunk.as_ptr(), chunk.len() as u32),
                    0
                );
            }
            assert_eq!(halls_digest_finish(digest, signature.as_mut_ptr()), 0);
            assert_ne!(halls_digest_finish(digest, signature.as_mut_ptr()), 0);
            assert_ne!(halls_digest_update(digest, raw.as_ptr(), 1), 0);
            halls_digest_close(digest);
        }
        let mut sealed = vec![0; 308 + size];
        unsafe {
            let state = halls_envelope_open(size as u32, size as u32 + 64, sealed.as_mut_ptr());
            assert!(!state.is_null());
            assert_ne!(halls_envelope_finish(state), 0);
            for (i, chunk) in raw.chunks(4096).enumerate() {
                assert_eq!(
                    halls_envelope_update(
                        state,
                        chunk.as_ptr(),
                        chunk.len() as u32,
                        sealed[308 + i * 4096..].as_mut_ptr()
                    ),
                    0
                );
            }
            assert_eq!(halls_envelope_finish(state), 0);
            assert_ne!(halls_envelope_finish(state), 0);
            assert_ne!(
                halls_envelope_update(state, raw.as_ptr(), 1, sealed[308..].as_mut_ptr()),
                0
            );
            halls_envelope_close(state);
        }
        std::fs::write(folder.join(format!("{size}.sig")), signature).unwrap();
        std::fs::write(folder.join(format!("{size}.enc")), sealed).unwrap();
    }
    println!("Native stream boundaries and lifecycle passed");
}
