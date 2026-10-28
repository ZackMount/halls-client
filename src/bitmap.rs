//! One bit per tile; construction, carving and collision share the same storage.
#[allow(unused_imports)]
use crate::prelude::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walls {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) bits: Vec<u64>,
}
impl Walls {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            bits: vec![u64::MAX; (width * height).div_ceil(64)],
        }
    }
    pub fn get(&self, x: usize, z: usize) -> bool {
        let index = z * self.width + x;
        self.bits[index >> 6] & (1 << (index & 63)) != 0
    }
    pub fn set(&mut self, x: usize, z: usize, solid: bool) {
        let index = z * self.width + x;
        let mask = 1 << (index & 63);
        if solid {
            self.bits[index >> 6] |= mask;
        } else {
            self.bits[index >> 6] &= !mask;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn word_and_row_boundaries_are_independent() {
        let mut walls = Walls::new(129, 129);
        assert_eq!(walls.bits.len() * 8, 2088);
        for z in 0..129 {
            for x in 0..129 {
                walls.set(x, z, (x + z) % 3 == 0);
            }
        }
        for z in 0..129 {
            for x in 0..129 {
                assert_eq!(walls.get(x, z), (x + z) % 3 == 0);
            }
        }
        walls.set(128, 128, true);
        walls.set(0, 1, false);
        assert!(walls.get(128, 128));
        assert!(!walls.get(0, 1));
        assert!(walls.get(0, 0));
    }
}
