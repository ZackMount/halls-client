#[allow(unused_imports)]
use crate::prelude::*;
use core::ops::{Add, Mul};
#[cfg(not(target_arch = "x86_64"))]
use core::ops::{Div, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}
impl Add for Vec2 {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, b: f32) -> Self {
        Self::new(self.x * b, self.y * b)
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod scalar {
    use super::*;
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Vec3 {
        pub x: f32,
        pub y: f32,
        pub z: f32,
    }
    impl Vec3 {
        pub const fn new(x: f32, y: f32, z: f32) -> Self {
            Self { x, y, z }
        }
        #[inline(never)]
        pub fn dot(self, b: Self) -> f32 {
            self.x * b.x + self.y * b.y + self.z * b.z
        }
        #[inline(never)]
        pub fn cross(self, b: Self) -> Self {
            Self::new(
                self.y * b.z - self.z * b.y,
                self.z * b.x - self.x * b.z,
                self.x * b.y - self.y * b.x,
            )
        }
        pub fn length(self) -> f32 {
            self.dot(self).sqrt()
        }
        #[inline(never)]
        pub fn normalized(self) -> Self {
            self / self.length().max(1e-8)
        }
        #[inline(never)]
        pub fn lerp(self, b: Self, t: f32) -> Self {
            self * (1.0 - t) + b * t
        }
    }
    impl Add for Vec3 {
        type Output = Self;
        fn add(self, b: Self) -> Self {
            Self::new(self.x + b.x, self.y + b.y, self.z + b.z)
        }
    }
    impl Sub for Vec3 {
        type Output = Self;
        fn sub(self, b: Self) -> Self {
            Self::new(self.x - b.x, self.y - b.y, self.z - b.z)
        }
    }
    impl Mul<f32> for Vec3 {
        type Output = Self;
        fn mul(self, b: f32) -> Self {
            Self::new(self.x * b, self.y * b, self.z * b)
        }
    }
    impl Div<f32> for Vec3 {
        type Output = Self;
        fn div(self, b: f32) -> Self {
            self * (1.0 / b)
        }
    }
}
#[cfg(not(target_arch = "x86_64"))]
pub use scalar::Vec3;

#[cfg(target_arch = "x86_64")]
mod simd {
    #[allow(unused_imports)]
    use crate::prelude::*;
    use core::{
        arch::x86_64::*,
        ops::{Add, Deref, DerefMut, Div, Mul, Sub},
    };
    #[repr(C)]
    pub struct Fields {
        pub x: f32,
        pub y: f32,
        pub z: f32,
        _padding: f32,
    }
    #[repr(transparent)]
    #[derive(Clone, Copy)]
    pub struct Vec3(__m128);
    // SSE/SSE2 are baseline x86-64 features. These layout assertions also
    // document the field-view casts used by Deref and DerefMut.
    const _: [(); 16] = [(); core::mem::size_of::<Fields>()];
    const _: [(); 16] = [(); core::mem::size_of::<Vec3>()];
    impl Deref for Vec3 {
        type Target = Fields;
        fn deref(&self) -> &Fields {
            // Both layouts contain four initialized f32 lanes; __m128 is more
            // strongly aligned. Borrowing preserves the original xyz field API.
            unsafe { &*(core::ptr::from_ref(&self.0).cast::<Fields>()) }
        }
    }
    impl DerefMut for Vec3 {
        fn deref_mut(&mut self) -> &mut Fields {
            unsafe { &mut *(core::ptr::from_mut(&mut self.0).cast::<Fields>()) }
        }
    }
    impl Default for Vec3 {
        fn default() -> Self {
            Self::new(0.0, 0.0, 0.0)
        }
    }
    impl PartialEq for Vec3 {
        fn eq(&self, b: &Self) -> bool {
            self.x == b.x && self.y == b.y && self.z == b.z
        }
    }
    impl core::fmt::Debug for Vec3 {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("Vec3")
                .field("x", &self.x)
                .field("y", &self.y)
                .field("z", &self.z)
                .finish()
        }
    }
    impl Vec3 {
        pub fn new(x: f32, y: f32, z: f32) -> Self {
            unsafe { Self(_mm_set_ps(0.0, z, y, x)) }
        }
        pub fn dot(self, b: Self) -> f32 {
            unsafe {
                let p = _mm_mul_ps(self.0, b.0);
                let xy = _mm_add_ss(p, _mm_shuffle_ps::<0x55>(p, p));
                _mm_cvtss_f32(_mm_add_ss(xy, _mm_shuffle_ps::<0xaa>(p, p)))
            }
        }
        pub fn cross(self, b: Self) -> Self {
            unsafe {
                Self(_mm_sub_ps(
                    _mm_mul_ps(
                        _mm_shuffle_ps::<0xc9>(self.0, self.0),
                        _mm_shuffle_ps::<0xd2>(b.0, b.0),
                    ),
                    _mm_mul_ps(
                        _mm_shuffle_ps::<0xd2>(self.0, self.0),
                        _mm_shuffle_ps::<0xc9>(b.0, b.0),
                    ),
                ))
            }
        }
        pub fn length(self) -> f32 {
            unsafe { _mm_cvtss_f32(_mm_sqrt_ss(_mm_set_ss(self.dot(self)))) }
        }
        pub fn normalized(self) -> Self {
            self / self.length().max(1e-8)
        }
        pub fn lerp(self, b: Self, t: f32) -> Self {
            self * (1.0 - t) + b * t
        }
    }
    impl Add for Vec3 {
        type Output = Self;
        fn add(self, b: Self) -> Self {
            unsafe { Self(_mm_add_ps(self.0, b.0)) }
        }
    }
    impl Sub for Vec3 {
        type Output = Self;
        fn sub(self, b: Self) -> Self {
            unsafe { Self(_mm_sub_ps(self.0, b.0)) }
        }
    }
    impl Mul<f32> for Vec3 {
        type Output = Self;
        fn mul(self, b: f32) -> Self {
            unsafe { Self(_mm_mul_ps(self.0, _mm_set1_ps(b))) }
        }
    }
    impl Div<f32> for Vec3 {
        type Output = Self;
        fn div(self, b: f32) -> Self {
            self * (1.0 / b)
        }
    }
}
#[cfg(target_arch = "x86_64")]
pub use simd::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub fov: f32,
}
impl Camera {
    pub fn forward(self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }
    pub fn right(self) -> Vec3 {
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        right * self.roll.cos() + self.forward().cross(right) * self.roll.sin()
    }
    pub fn up(self) -> Vec3 {
        self.forward().cross(self.right())
    }
    pub fn to_view(self, world: Vec3) -> Vec3 {
        let d = world - self.position;
        Vec3::new(d.dot(self.right()), d.dot(self.up()), d.dot(self.forward()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn simd_xyz_matches_scalar_formulas_and_mutation() {
        assert_eq!(core::mem::size_of::<Vec3>(), 16);
        assert_eq!(core::mem::align_of::<Vec3>(), 16);
        let mut random = 42u32;
        for _ in 0..4096 {
            let mut next = || {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                ((random >> 8) as f32 / 65536.0) - 128.0
            };
            let a = Vec3::new(next(), next(), next());
            let b = Vec3::new(next(), next(), next());
            let components = |v: Vec3| [v.x, v.y, v.z];
            assert_eq!(components(a + b), [a.x + b.x, a.y + b.y, a.z + b.z]);
            assert_eq!(components(a - b), [a.x - b.x, a.y - b.y, a.z - b.z]);
            assert_eq!(components(a * 0.37), [a.x * 0.37, a.y * 0.37, a.z * 0.37]);
            assert_eq!(a.dot(b), a.x * b.x + a.y * b.y + a.z * b.z);
            assert_eq!(
                components(a.cross(b)),
                [
                    a.y * b.z - a.z * b.y,
                    a.z * b.x - a.x * b.z,
                    a.x * b.y - a.y * b.x
                ]
            );
            assert_eq!(a.length(), (a.x * a.x + a.y * a.y + a.z * a.z).sqrt());
            let mut changed = a;
            changed.x = b.x;
            changed.y += 1.0;
            changed.z = -b.z;
            assert_eq!(components(changed), [b.x, a.y + 1.0, -b.z]);
        }
    }
    #[test]
    fn camera_basis_remains_orthonormal_with_pitch() {
        let c = Camera {
            position: Vec3::default(),
            yaw: 1.12,
            pitch: 0.61,
            roll: 0.0,
            fov: 1.4,
        };
        assert!(c.forward().dot(c.right()).abs() < 1e-6);
        assert!(c.forward().dot(c.up()).abs() < 1e-6);
        assert!((c.up().length() - 1.0).abs() < 1e-6);
        let p = c.to_view(c.forward() * 3.0);
        assert!(p.x.abs() < 1e-6 && p.y.abs() < 1e-6 && (p.z - 3.0).abs() < 1e-6);
    }
}
