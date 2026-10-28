//! No-std fatal-exit fixture; exercises the same abort path as the player binary.
#![cfg_attr(all(not(any(feature = "tools", test)), panic = "abort"), no_std)]
#![cfg_attr(all(not(any(feature = "tools", test)), panic = "abort"), no_main)]
#[cfg(all(not(any(feature = "tools", test)), panic = "abort"))]
#[global_allocator]
static ALLOCATOR: halls::runtime::Allocator = halls::runtime::Allocator;
#[cfg(all(not(any(feature = "tools", test)), panic = "abort"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    halls::runtime::exit(1)
}
#[cfg(all(not(any(feature = "tools", test)), panic = "abort"))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let _terminal = halls::terminal::Terminal::enter().unwrap();
    let _ = halls::console::text(b"HALLS: fatal test\r\n");
    panic!("fatal test");
}
#[cfg(all(not(any(feature = "tools", test)), panic = "abort"))]
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() -> ! {
    halls::runtime::exit(1)
}

#[cfg(any(feature = "tools", test, panic = "unwind"))]
fn main() {
    let _terminal = halls::terminal::Terminal::enter().unwrap();
    let _ = halls::console::text(b"HALLS: fatal test\r\n");
    halls::runtime::exit(1);
}
