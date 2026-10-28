//! Small buffered stdout sink. The OS adapter handles redirected output and UTF-8.
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::prelude::*;
unsafe extern "C" {
    fn halls_write(channel: i32, bytes: *const u8, length: usize) -> i32;
}
fn send(channel: i32, bytes: &[u8]) -> Result<()> {
    // The synchronous adapter reads the complete live slice before returning.
    if unsafe { halls_write(channel, bytes.as_ptr(), bytes.len()) } == 0 {
        Ok(())
    } else {
        Err(Error::new("Console write failed".into()))
    }
}
#[derive(Default)]
pub struct Buffer(Vec<u8>);
impl Buffer {
    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.0.extend_from_slice(value);
        self
    }
    pub fn number(&mut self, value: usize) -> &mut Self {
        self.padded_number(value, 0, b' ')
    }
    pub fn into_text(self) -> String {
        String::from_utf8(self.0).unwrap()
    }
    pub fn padded_number(&mut self, mut value: usize, width: usize, pad: u8) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.0
            .resize(self.0.len() + width.saturating_sub(digits.len() - at), pad);
        self.bytes(&digits[at..])
    }
}
impl Buffer {
    pub fn flush(&mut self) -> Result<()> {
        send(1, &self.0)?;
        self.0.clear();
        Ok(())
    }
}
pub fn error(message: &str) {
    let _ = send(2, b"HALLS: ");
    let _ = send(2, message.as_bytes());
    let _ = send(2, b"\n");
}
pub fn text(bytes: &[u8]) -> Result<()> {
    send(1, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn padded_hud_numbers_preserve_width_and_utf8() {
        for value in [0, 1, 9, 10, 59, 60, 99, 100, 1000] {
            let mut text = Buffer::default();
            text.padded_number(value, 2, b'0');
            assert_eq!(text.into_text(), format!("{value:02}"));
            let mut text = Buffer::default();
            text.padded_number(value, 3, b' ');
            assert_eq!(text.into_text(), format!("{value:3}"));
        }
        let mut text = Buffer::default();
        text.bytes("HIT — MOVE!".as_bytes());
        assert_eq!(text.into_text(), "HIT — MOVE!");
    }
    #[test]
    fn terminal_numbers_match_decimal_formatting() {
        for value in (0..10000).chain([usize::MAX, usize::MAX / 10]) {
            let mut buffer = Buffer::default();
            buffer.number(value);
            assert_eq!(buffer.0, value.to_string().as_bytes());
        }
    }
}
