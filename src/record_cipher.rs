//! Per-run memory barrier, not an authentication boundary against a process owner.
//! ChaCha20 (RFC 8439); proof signing and authenticated export remain independent.
use crate::world::fresh_seed;

pub(crate) fn wipe<T: Copy + Default>(data: &mut [T]) {
    for value in data {
        unsafe { core::ptr::write_volatile(value, T::default()) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}
pub(crate) struct Cipher {
    key: [u32; 8],
    nonce: [u32; 3],
}
impl Cipher {
    pub fn new() -> Self {
        let mut key = [0; 8];
        for pair in key.chunks_mut(2) {
            let n = fresh_seed();
            pair[0] = n as u32;
            pair[1] = (n >> 32) as u32;
        }
        let a = fresh_seed();
        Self {
            key,
            nonce: [a as u32, (a >> 32) as u32, fresh_seed() as u32],
        }
    }
    #[inline(always)]
    fn block(&self, counter: u32, domain: u32) -> [u8; 64] {
        let mut initial = [0u32; 16];
        initial[..4].copy_from_slice(&[0x61707865, 0x3320646e, 0x79622d32, 0x6b206574]);
        initial[4..12].copy_from_slice(&self.key);
        initial[12] = counter;
        initial[13..].copy_from_slice(&self.nonce);
        initial[13] ^= domain;
        let mut x = initial;
        for _ in 0..10 {
            for [a, b, c, d] in [
                [0, 4, 8, 12],
                [1, 5, 9, 13],
                [2, 6, 10, 14],
                [3, 7, 11, 15],
                [0, 5, 10, 15],
                [1, 6, 11, 12],
                [2, 7, 8, 13],
                [3, 4, 9, 14],
            ] {
                x[a] = x[a].wrapping_add(x[b]);
                x[d] = (x[d] ^ x[a]).rotate_left(16);
                x[c] = x[c].wrapping_add(x[d]);
                x[b] = (x[b] ^ x[c]).rotate_left(12);
                x[a] = x[a].wrapping_add(x[b]);
                x[d] = (x[d] ^ x[a]).rotate_left(8);
                x[c] = x[c].wrapping_add(x[d]);
                x[b] = (x[b] ^ x[c]).rotate_left(7);
            }
        }
        let mut bytes = [0; 64];
        for i in 0..16 {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&x[i].wrapping_add(initial[i]).to_le_bytes());
        }
        wipe(&mut x);
        wipe(&mut initial);
        bytes
    }
    #[inline(always)]
    pub fn bytes(&self, data: &mut [u8], offset: usize) {
        let end = (offset as u64)
            .checked_add(data.len() as u64)
            .expect("Recording size");
        assert!(end <= (u32::MAX as u64 + 1) * 64, "Recording size");
        let mut at = 0;
        while at < data.len() {
            let position = offset + at;
            let mut block = self.block((position / 64) as u32, 0);
            let skip = position % 64;
            let size = (64 - skip).min(data.len() - at);
            for i in 0..size {
                data[at + i] ^= block[skip + i];
            }
            wipe(&mut block);
            at += size;
        }
    }
    #[inline(always)]
    pub fn words(&self, data: &mut [u32], frame: u32) {
        let stride = data.len().div_ceil(16) as u64;
        let counter = frame as u64 * stride;
        assert!(counter + stride <= u32::MAX as u64, "Recording frame limit");
        for (i, chunk) in data.chunks_mut(16).enumerate() {
            let mut block = self.block((counter + i as u64) as u32, 1);
            for (j, word) in chunk.iter_mut().enumerate() {
                *word ^= u32::from_le_bytes(block[j * 4..j * 4 + 4].try_into().unwrap());
            }
            wipe(&mut block);
        }
    }
}
impl Drop for Cipher {
    fn drop(&mut self) {
        wipe(&mut self.key);
        wipe(&mut self.nonce);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc8439_block_vector() {
        let cipher = Cipher {
            key: core::array::from_fn(|i| {
                u32::from_le_bytes(core::array::from_fn(|j| (i * 4 + j) as u8))
            }),
            nonce: [0x09000000, 0x4a000000, 0],
        };
        let hex: alloc::string::String = cipher
            .block(1, 0)
            .iter()
            .map(|b| alloc::format!("{b:02x}"))
            .collect();
        assert_eq!(
            hex,
            "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4ed2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
        );
    }
    #[test]
    fn offsets_domains_and_frame_nonces_are_independent() {
        let c = Cipher::new();
        let mut full = [0u8; 197];
        c.bytes(&mut full, 0);
        let mut part = [0u8; 129];
        c.bytes(&mut part, 63);
        assert_eq!(&part, &full[63..192]);
        c.bytes(&mut part, 63);
        assert_eq!(part, [0; 129]);
        let mut first = [0u32; 85];
        let mut next = first;
        c.words(&mut first, 0);
        c.words(&mut next, 1);
        assert_ne!(first, next);
        assert_ne!(first[0].to_le_bytes(), full[..4]);
        c.words(&mut first, 0);
        assert_eq!(first, [0; 85]);
    }
}
