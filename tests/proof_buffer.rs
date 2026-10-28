use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK_REALLOCS: Cell<bool> = const { Cell::new(false) };
    static REALLOCS: Cell<usize> = const { Cell::new(0) };
}

struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if TRACK_REALLOCS.try_with(Cell::get).unwrap_or(false) {
            let _ = REALLOCS.try_with(|count| count.set(count.get() + 1));
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

struct Track;

impl Track {
    fn start() -> Self {
        REALLOCS.with(|count| count.set(0));
        TRACK_REALLOCS.with(|active| active.set(true));
        Self
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        TRACK_REALLOCS.with(|active| active.set(false));
    }
}

// Decode the documented HLC1 wire format, including overlapping backreferences.
fn decompress(input: &[u8], expected_size: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(expected_size);
    let mut at = 0;
    while output.len() < expected_size {
        let flags = *input.get(at).expect("missing token flags");
        at += 1;
        for bit in 0..8 {
            if output.len() == expected_size {
                break;
            }
            if flags & (1 << bit) == 0 {
                output.push(*input.get(at).expect("missing literal"));
                at += 1;
            } else {
                let bytes = input.get(at..at + 2).expect("missing backreference");
                let token = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
                at += 2;
                let distance = (token >> 5) + 1;
                let length = (token & 31) + 3;
                assert!(distance <= output.len(), "backreference before start");
                assert!(length <= expected_size - output.len(), "extra output");
                for _ in 0..length {
                    output.push(output[output.len() - distance]);
                }
            }
        }
    }
    assert_eq!(at, input.len(), "unused compressed bytes");
    output
}

fn verify(input: &[u8], kind: &str) {
    let track = Track::start();
    let compressed = halls::proof::compress(input);
    drop(track);
    assert_eq!(
        REALLOCS.with(Cell::get),
        0,
        "compression reallocated a plaintext buffer: {kind}, {} bytes",
        input.len()
    );
    assert_eq!(decompress(&compressed, input.len()), input, "{kind}");
}

#[test]
fn compression_never_reallocates_plaintext_and_roundtrips() {
    // One test owns the tracking interval; thread-local counters exclude harness
    // activity on other threads. Construct inputs and decode after tracking ends.
    let lengths = [
        0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 34, 35, 63, 64, 65, 2047, 2048, 2049, 4095,
        4096, 4097, 65536, 131071,
    ];
    let mut state = 0x1234_5678_9abc_def0u64;
    for length in lengths {
        let mut random = Vec::with_capacity(length);
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            random.push((state >> 32) as u8);
        }
        verify(&random, "random");
        verify(&vec![0; length], "zero");
        let repeated: Vec<u8> = b"HLR4-proof-packet\0"
            .iter()
            .copied()
            .cycle()
            .take(length)
            .collect();
        verify(&repeated, "repeated");
    }
}
