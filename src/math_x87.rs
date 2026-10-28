//! Scalar x86-64 math for the fixed game. Every x87 sequence leaves an empty
//! register stack; all eight x87 registers are declared clobbered. No CRT calls.
use core::arch::asm;
macro_rules! unary {
    ($name:ident, $($op:literal),+) => {
        pub fn $name(x:f32)->f32 {
            let mut result=0.0f32;
            unsafe {asm!("fld dword ptr [{x}]",$($op,)+"fstp dword ptr [{out}]",
                x=in(reg)&x,out=in(reg)&mut result,
                out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,
                out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,
                options(nostack,preserves_flags));}
            result
        }
    }
}
unary!(sin, "fsin");
unary!(cos, "fcos");
unary!(tan, "fptan", "fstp st(0)");
unary!(sqrt, "fsqrt");
fn integer(x: f64, mode: u16) -> f64 {
    let mut result = 0.0f64;
    let mut old = 0u16;
    unsafe {
        asm!("fnstcw word ptr [{cw}]",cw=in(reg)&mut old,options(nostack,preserves_flags));
    }
    let control = (old & !0x0c00) | mode;
    unsafe {
        asm!("fldcw word ptr [{new}]","fld qword ptr [{x}]","frndint",
        "fstp qword ptr [{out}]","fldcw word ptr [{old}]",
        new=in(reg)&control,old=in(reg)&old,x=in(reg)&x,out=in(reg)&mut result,
        out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,
        out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,
        options(nostack,preserves_flags));
    }
    result
}
pub fn floor(x: f32) -> f32 {
    integer(x as f64, 0x400) as f32
}
pub fn ceil(x: f32) -> f32 {
    integer(x as f64, 0x800) as f32
}
pub fn trunc(x: f32) -> f32 {
    integer(x as f64, 0xc00) as f32
}
pub fn trunc64(x: f64) -> f64 {
    integer(x, 0xc00)
}
pub fn round(x: f32) -> f32 {
    let n = trunc(x);
    if (x - n).abs() >= 0.5 {
        n + if x < 0.0 { -1.0 } else { 1.0 }
    } else {
        n
    }
}
pub fn atan2(y: f32, x: f32) -> f32 {
    let mut result = 0.0;
    unsafe {
        asm!("fld dword ptr [{y}]","fld dword ptr [{x}]","fpatan","fstp dword ptr [{out}]",
        x=in(reg)&x,y=in(reg)&y,out=in(reg)&mut result,
        out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,
        out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,
        options(nostack,preserves_flags));
    }
    result
}
pub fn hypot(x: f32, y: f32) -> f32 {
    let mut result = 0.0;
    unsafe {
        asm!("fld dword ptr [{x}]","fmul st(0),st(0)","fld dword ptr [{y}]",
        "fmul st(0),st(0)","faddp st(1),st(0)","fsqrt","fstp dword ptr [{out}]",
        x=in(reg)&x,y=in(reg)&y,out=in(reg)&mut result,
        out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,
        out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,
        options(nostack,preserves_flags));
    }
    result
}
macro_rules! exponential {
    ($x:expr,$y:expr,$($setup:literal),+) => {{
        let x=$x;let y=$y;let mut result=0.0f32;
        unsafe {asm!($($setup,)+
            "fld st(0)","frndint","fxch st(1)","fsub st(0),st(1)",
            "f2xm1","fld1","faddp st(1),st(0)","fscale","fstp st(1)","fstp dword ptr [{out}]",
            x=in(reg)&x,y=in(reg)&y,out=in(reg)&mut result,
            out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,
            out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,
            options(nostack,preserves_flags));}
        result
    }}
}
pub fn exp(x: f32) -> f32 {
    // The renderer calls exp only with nonpositive finite fog exponents.
    exponential!(
        x,
        core::f32::consts::LOG2_E,
        "fld dword ptr [{x}]",
        "fld dword ptr [{y}]",
        "fmulp st(1),st(0)"
    )
}
pub fn pow(x: f32, y: f32) -> f32 {
    // All powf call sites use nonnegative bases (gamma, cable shape, envelopes).
    if x == 0.0 {
        return if y == 0.0 {
            1.0
        } else if y > 0.0 {
            0.0
        } else {
            f32::INFINITY
        };
    }
    exponential!(x, y, "fld dword ptr [{y}]", "fld dword ptr [{x}]", "fyl2x")
}

#[cfg_attr(windows, unsafe(no_mangle))]
pub extern "C" fn fmodf(x: f32, divisor: f32) -> f32 {
    if !x.is_finite() || divisor == 0.0 || divisor.is_nan() {
        return f32::NAN;
    }
    if divisor.is_infinite() {
        return x;
    }
    let mut result = 0.0;
    unsafe {
        asm!("fld dword ptr [{d}]","fld dword ptr [{x}]",
        "2:","fprem","fnstsw ax","test ah,4","jnz 2b",
        "fstp dword ptr [{out}]","fstp st(0)",
        x=in(reg)&x,d=in(reg)&divisor,out=in(reg)&mut result,out("ax") _,
        out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,
        out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,options(nostack));
    }
    result
}

pub fn sin_cos(x: f32) -> (f32, f32) {
    let (mut sin, mut cos) = (0.0f32, 0.0f32);
    unsafe {
        asm!("fld dword ptr [{x}]","fsincos","fstp dword ptr [{c}]","fstp dword ptr [{s}]",
 x=in(reg)&x,c=in(reg)&mut cos,s=in(reg)&mut sin,
 out("st(0)") _,out("st(1)") _,out("st(2)") _,out("st(3)") _,out("st(4)") _,out("st(5)") _,out("st(6)") _,out("st(7)") _,options(nostack,preserves_flags));
    }
    (sin, cos)
}
