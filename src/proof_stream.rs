//! Bounded export scratch: the complete recording and compressed staging remain
//! encrypted. This is not remote attestation.
use super::{Cipher, Plain, wipe};
use crate::prelude::*;
use core::ffi::c_void;

const CHUNK: usize = 4096;
unsafe extern "C" {
    fn halls_digest_open() -> *mut c_void;
    fn halls_digest_update(state: *mut c_void, data: *const u8, size: u32) -> i32;
    fn halls_digest_finish(state: *mut c_void, signature: *mut u8) -> i32;
    fn halls_digest_close(state: *mut c_void);
    fn halls_envelope_open(size: u32, plain_size: u32, header: *mut u8) -> *mut c_void;
    fn halls_envelope_update(state: *mut c_void, data: *const u8, size: u32, out: *mut u8) -> i32;
    fn halls_envelope_finish(state: *mut c_void) -> i32;
    fn halls_envelope_close(state: *mut c_void);
}
struct Digest(*mut c_void);
impl Drop for Digest {
    fn drop(&mut self) {
        unsafe { halls_digest_close(self.0) }
    }
}
struct Envelope(*mut c_void);
impl Drop for Envelope {
    fn drop(&mut self) {
        unsafe { halls_envelope_close(self.0) }
    }
}
#[inline(always)]
pub(super) fn signature(data: &[u8], cipher: &Cipher) -> core::result::Result<[u8; 64], i32> {
    let state = Digest(unsafe { halls_digest_open() });
    if state.0.is_null() {
        return Err(-2);
    }
    let mut scratch = Plain(vec![0; CHUNK]);
    for (i, chunk) in data.chunks(CHUNK).enumerate() {
        let bytes = &mut scratch.0[..chunk.len()];
        bytes.copy_from_slice(chunk);
        cipher.bytes(bytes, i * CHUNK);
        let result = unsafe { halls_digest_update(state.0, bytes.as_ptr(), bytes.len() as u32) };
        wipe(bytes);
        if result != 0 {
            return Err(-2);
        }
    }
    let mut signature = [0; 64];
    if unsafe { halls_digest_finish(state.0, signature.as_mut_ptr()) } != 0 {
        wipe(&mut signature);
        return Err(-2);
    }
    Ok(signature)
}

// Two pages cover the 2048-byte HLC history plus its 34-byte lookahead. Page
// replacement never reallocates plaintext and drops wipe both pages on errors.
struct Reader<'a> {
    data: &'a [u8],
    cipher: &'a Cipher,
    signature: &'a [u8; 64],
    pages: [usize; 2],
    cache: Plain,
}
impl Reader<'_> {
    #[inline(always)]
    fn byte(&mut self, at: usize) -> u8 {
        if at >= self.data.len() {
            return self.signature[at - self.data.len()];
        }
        let page = at / CHUNK;
        let slot = page % 2;
        if self.pages[slot] != page {
            let offset = page * CHUNK;
            let size = (self.data.len() - offset).min(CHUNK);
            let buffer = &mut self.cache.0[slot * CHUNK..(slot + 1) * CHUNK];
            wipe(buffer);
            buffer[..size].copy_from_slice(&self.data[offset..offset + size]);
            self.cipher.bytes(&mut buffer[..size], offset);
            self.pages[slot] = page;
        }
        self.cache.0[slot * CHUNK + at % CHUNK]
    }
}
#[inline(always)]
pub(super) fn pack(data: &[u8], cipher: &Cipher, signature: &[u8; 64], output: &Cipher) -> Vec<u8> {
    let size = data.len() + signature.len();
    let mut reader = Reader {
        data,
        cipher,
        signature,
        pages: [usize::MAX; 2],
        cache: Plain(vec![0; CHUNK * 2]),
    };
    let mut table = vec![usize::MAX; 4096];
    let mut out = Vec::with_capacity(size + size.div_ceil(8));
    let mut at = 0;
    while at < size {
        let mut group = [0u8; 17];
        let mut used = 1;
        for bit in 0..8 {
            if at == size {
                break;
            }
            let mut length = 0;
            let mut distance = 0;
            if at + 2 < size {
                let hash = (reader.byte(at) as usize * 251
                    + reader.byte(at + 1) as usize * 31
                    + reader.byte(at + 2) as usize)
                    & 4095;
                let from = table[hash];
                table[hash] = at;
                if from < at && at - from <= 2048 {
                    while length < 34
                        && at + length < size
                        && reader.byte(from + length) == reader.byte(at + length)
                    {
                        length += 1;
                    }
                    distance = at - from;
                }
            }
            if length >= 3 {
                group[0] |= 1 << bit;
                group[used..used + 2].copy_from_slice(
                    &((((distance - 1) << 5) | (length - 3)) as u16).to_le_bytes(),
                );
                used += 2;
                at += length;
            } else {
                group[used] = reader.byte(at);
                used += 1;
                at += 1;
            }
        }
        output.bytes(&mut group[..used], out.len());
        out.extend_from_slice(&group[..used]);
        wipe(&mut group);
    }
    out
}
#[inline(always)]
pub(super) fn seal(
    data: &[u8],
    cipher: &Cipher,
    plain_size: u32,
) -> core::result::Result<Vec<u8>, i32> {
    if data.len() > u32::MAX as usize - 308 {
        return Err(-1);
    }
    let mut out = vec![0; 308 + data.len()];
    let state =
        Envelope(unsafe { halls_envelope_open(data.len() as u32, plain_size, out.as_mut_ptr()) });
    if state.0.is_null() {
        return Err(-2);
    }
    let mut scratch = Plain(vec![0; CHUNK]);
    for (i, chunk) in data.chunks(CHUNK).enumerate() {
        let bytes = &mut scratch.0[..chunk.len()];
        bytes.copy_from_slice(chunk);
        cipher.bytes(bytes, i * CHUNK);
        let result = unsafe {
            halls_envelope_update(
                state.0,
                bytes.as_ptr(),
                bytes.len() as u32,
                out[308 + i * CHUNK..].as_mut_ptr(),
            )
        };
        wipe(bytes);
        if result != 0 {
            return Err(-2);
        }
    }
    if unsafe { halls_envelope_finish(state.0) } != 0 {
        return Err(-2);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_stream_compression_matches_reference_at_page_and_overlap_boundaries() {
        for n in [0, 1, 2, 33, 2047, 2048, 4095, 4096, 4097, 8193, 65539] {
            for pattern in 0..3 {
                let plain: Vec<u8> = (0..n)
                    .map(|i| match pattern {
                        0 => 7,
                        1 => (i % 251) as u8,
                        _ => ((i * 73) ^ (i / 13) ^ (i * i / 193)) as u8,
                    })
                    .collect();
                let signature = core::array::from_fn(|i| i as u8);
                let mut signed = plain.clone();
                signed.extend_from_slice(&signature);
                let expected = super::super::compress(&signed);
                let source = Cipher::new();
                let output = Cipher::new();
                let mut encrypted = plain;
                source.bytes(&mut encrypted, 0);
                let mut packed = pack(&encrypted, &source, &signature, &output);
                output.bytes(&mut packed, 0);
                assert_eq!(packed, expected, "length {n}, pattern {pattern}");
            }
        }
    }
}
