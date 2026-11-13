//! Angles and the circle functions every part of Melon shares: vanilla's

use glam::{Mat3, Mat4, Quat, Vec3};

pub use std::f32::consts::FRAC_PI_2 as HALF_PI;
pub use std::f32::consts::FRAC_PI_3 as THIRD_PI;
pub use std::f32::consts::FRAC_PI_4 as QUARTER_PI;
pub use std::f32::consts::FRAC_PI_6 as SIXTH_PI;
pub use std::f32::consts::FRAC_PI_8 as EIGHTH_PI;
pub use std::f32::consts::PI;
pub use std::f32::consts::TAU;

/// The same in `f64`, for vanilla's `Math.PI` sums in doubles.
pub mod double {
    pub use std::f64::consts::FRAC_PI_2 as HALF_PI;
    pub use std::f64::consts::PI;
    pub use std::f64::consts::TAU;
}

/// `Mth.DEG_TO_RAD`.
pub const DEG_TO_RAD: f32 = PI / 180.0;
/// `Mth.RAD_TO_DEG`.
pub const RAD_TO_DEG: f32 = 180.0 / PI;

/// Vanilla's steps around the circle (`Mth.SIN.length`).
const STEPS: i64 = 1 << 16;
/// `65536 / 2π`, vanilla's literal `10430.378F`.
const STEPS_PER_RADIAN: f32 = 10430.378;
/// A quarter turn in steps: the cosine is the sine this far on.
const QUARTER: f32 = 16384.0;
/// Steps between two entries of [`COARSE`].
const FINE_BITS: u32 = 8;
const COARSE_LEN: usize = 1 << (16 - FINE_BITS);
const FINE_LEN: usize = 1 << FINE_BITS;
/// One step in radians.
const STEP: f64 = double::TAU / STEPS as f64;

/// `sin(x)` for `0 <= x <= π/2`, Taylor series to `x^23`: below `1e-17`
const fn sin_quarter(x: f64) -> f64 {
    let x2 = x * x;
    let mut sum = 0.0;
    let mut n = 23;
    while n > 1 {
        sum = 1.0 - x2 / ((n * (n - 1)) as f64) * sum;
        n -= 2;
    }
    x * sum
}

/// `cos(x)` for small `x` (under a 256th of a turn), Taylor series to
const fn cos_small(x: f64) -> f64 {
    let x2 = x * x;
    let mut sum = 0.0;
    let mut n = 16;
    while n > 0 {
        sum = 1.0 - x2 / ((n * (n - 1)) as f64) * sum;
        n -= 2;
    }
    sum
}

/// A table on its own cache line, so none of it shares a line with
#[repr(C, align(64))]
struct Aligned<T>(T);

/// The sine at every 256th step, by quadrant symmetry from the first.
static COARSE: Aligned<[f64; COARSE_LEN]> = Aligned({
    let quarter = COARSE_LEN / 4;
    let step = double::TAU / COARSE_LEN as f64;
    let mut table = [0.0; COARSE_LEN];
    let mut i = 0;
    while i < COARSE_LEN {
        let within = i % quarter;
        table[i] = match i / quarter {
            0 => sin_quarter(within as f64 * step),
            1 => sin_quarter((quarter - within) as f64 * step),
            2 => -sin_quarter(within as f64 * step),
            _ => -sin_quarter((quarter - within) as f64 * step),
        };
        i += 1;
    }
    table[COARSE_LEN / 2] = 1.224_646_799_147_353_2e-16;
    table
});

/// Sine and cosine of the steps between two coarse entries, side by side
static FINE: Aligned<[[f64; 2]; FINE_LEN]> = Aligned({
    let mut table = [[0.0; 2]; FINE_LEN];
    let mut i = 0;
    while i < FINE_LEN {
        let d = i as f64 * STEP;
        table[i] = [sin_quarter(d), cos_small(d)];
        i += 1;
    }
    table
});

/// Sine and cosine of vanilla's step `index`: the coarse entry's sine and
#[inline]
fn step(index: i64) -> (f32, f32) {
    let index = (index & (STEPS - 1)) as usize;
    let coarse = index >> FINE_BITS;
    let [sin_d, cos_d] = FINE.0[index & (FINE_LEN - 1)];
    let sin_a = COARSE.0[coarse];
    let cos_a = COARSE.0[(coarse + COARSE_LEN / 4) & (COARSE_LEN - 1)];
    (
        (sin_a * cos_d + cos_a * sin_d) as f32,
        (cos_a * cos_d - sin_a * sin_d) as f32,
    )
}

/// `Mth.sin`.
#[inline]
pub fn sin(radians: f32) -> f32 {
    step((radians * STEPS_PER_RADIAN) as i64).0
}

/// `Mth.cos`: vanilla's own index, a quarter turn on before rounding.
#[inline]
pub fn cos(radians: f32) -> f32 {
    step((radians * STEPS_PER_RADIAN + QUARTER) as i64).0
}

/// [`sin`] and [`cos`] together. When the cosine's step is the sine's a
#[inline]
pub fn sin_cos(radians: f32) -> (f32, f32) {
    let scaled = radians * STEPS_PER_RADIAN;
    let sin_index = scaled as i64;
    let cos_index = (scaled + QUARTER) as i64;
    let (sin, cos) = step(sin_index);
    if cos_index == sin_index + QUARTER as i64 {
        (sin, cos)
    } else {
        (sin, step(cos_index).0)
    }
}

/// Sine and cosine of one and the same table step: on the unit circle,
#[inline]
pub fn unit(radians: f32) -> (f32, f32) {
    step((radians * STEPS_PER_RADIAN) as i64)
}

/// `(sin, cos)` of an angle in degrees, as vanilla's movement code
#[inline]
pub fn sin_cos_deg(degrees: f32) -> (f32, f32) {
    sin_cos(degrees * DEG_TO_RAD)
}

/// `Mth.wrapDegrees`: the same angle in `[-180, 180)`.
#[inline]
pub fn wrap_degrees(degrees: f32) -> f32 {
    let wrapped = degrees % 360.0;
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

/// Rotations about the axes and from Euler angles, with [`unit`]
pub trait Rotation: Sized {
    /// About X by `radians`.
    fn rot_x(radians: f32) -> Self;
    fn rot_y(radians: f32) -> Self;
    fn rot_z(radians: f32) -> Self;

    /// `Axis.XP.rotationDegrees`.
    #[inline]
    fn rot_x_deg(degrees: f32) -> Self {
        Self::rot_x(degrees * DEG_TO_RAD)
    }
    #[inline]
    fn rot_y_deg(degrees: f32) -> Self {
        Self::rot_y(degrees * DEG_TO_RAD)
    }
    #[inline]
    fn rot_z_deg(degrees: f32) -> Self {
        Self::rot_z(degrees * DEG_TO_RAD)
    }

    /// Euler angles `(x, y, z)` in radians, applied Z first, then Y, then
    fn euler_zyx(radians: Vec3) -> Self;
}

impl Rotation for Mat3 {
    #[inline]
    fn rot_x(radians: f32) -> Self {
        let (s, c) = unit(radians);
        Mat3::from_cols(Vec3::X, Vec3::new(0.0, c, s), Vec3::new(0.0, -s, c))
    }

    #[inline]
    fn rot_y(radians: f32) -> Self {
        let (s, c) = unit(radians);
        Mat3::from_cols(Vec3::new(c, 0.0, -s), Vec3::Y, Vec3::new(s, 0.0, c))
    }

    #[inline]
    fn rot_z(radians: f32) -> Self {
        let (s, c) = unit(radians);
        Mat3::from_cols(Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0), Vec3::Z)
    }

    #[inline]
    fn euler_zyx(radians: Vec3) -> Self {
        let pair = |r: f32| if r == 0.0 { (0.0, 1.0) } else { unit(r) };
        let (sx, cx) = pair(radians.x);
        let (sy, cy) = pair(radians.y);
        let (sz, cz) = pair(radians.z);
        Mat3::from_cols(
            Vec3::new(cz * cy, sz * cy, -sy),
            Vec3::new(cz * sy * sx - sz * cx, sz * sy * sx + cz * cx, cy * sx),
            Vec3::new(cz * sy * cx + sz * sx, sz * sy * cx - cz * sx, cy * cx),
        )
    }
}

impl Rotation for Mat4 {
    #[inline]
    fn rot_x(radians: f32) -> Self {
        Mat4::from_mat3(Mat3::rot_x(radians))
    }
    #[inline]
    fn rot_y(radians: f32) -> Self {
        Mat4::from_mat3(Mat3::rot_y(radians))
    }
    #[inline]
    fn rot_z(radians: f32) -> Self {
        Mat4::from_mat3(Mat3::rot_z(radians))
    }
    #[inline]
    fn euler_zyx(radians: Vec3) -> Self {
        Mat4::from_mat3(Mat3::euler_zyx(radians))
    }
}

/// A rotation of `radians` about `axis` (a unit vector), as a quaternion.
#[inline]
pub fn axis_angle(axis: Vec3, radians: f32) -> Quat {
    let (s, c) = unit(radians * 0.5);
    let v = axis * s;
    Quat::from_xyzw(v.x, v.y, v.z, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vanilla's table: `(float) Math.sin(i * 2π / 65536)`.
    fn vanilla(i: i64) -> f32 {
        ((i as f64) * double::PI * 2.0 / STEPS as f64).sin() as f32
    }

    #[test]
    fn every_step_is_vanillas() {
        for i in 0..STEPS {
            let (s, c) = step(i);
            assert_eq!(s, vanilla(i), "sin step {i}");
            assert_eq!(c, vanilla((i + 16384) & (STEPS - 1)), "cos step {i}");
        }
    }

    #[test]
    fn lookups_index_like_mth() {
        for k in -7200..=7200 {
            let x = k as f32 * 0.001_7;
            let vanilla_sin = vanilla((x * STEPS_PER_RADIAN) as i64 & (STEPS - 1));
            let vanilla_cos = vanilla((x * STEPS_PER_RADIAN + QUARTER) as i64 & (STEPS - 1));
            assert_eq!(sin(x), vanilla_sin, "sin {x}");
            assert_eq!(cos(x), vanilla_cos, "cos {x}");
            assert_eq!(sin_cos(x), (vanilla_sin, vanilla_cos), "sin_cos {x}");
            assert!((sin(x) - x.sin()).abs() < 1e-4);
        }
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(sin(HALF_PI), 1.0);
    }

    #[test]
    fn rotations_match_glams() {
        for (a, b, c) in [(0.3, -1.2, 2.5), (-2.0, 0.7, 0.1), (3.0, 3.0, -3.0)] {
            let euler = Mat3::euler_zyx(Vec3::new(a, b, c));
            let glam = Mat3::from_quat(Quat::from_euler(glam::EulerRot::ZYX, c, b, a));
            assert!(euler.abs_diff_eq(glam, 2e-4), "{euler} {glam}");
            assert!(Mat3::rot_x(a).abs_diff_eq(Mat3::from_rotation_x(a), 2e-4));
            assert!(Mat3::rot_y(b).abs_diff_eq(Mat3::from_rotation_y(b), 2e-4));
            assert!(Mat3::rot_z(c).abs_diff_eq(Mat3::from_rotation_z(c), 2e-4));
            let axis = Vec3::new(1.0, 2.0, -0.5).normalize();
            assert!(axis_angle(axis, a).abs_diff_eq(Quat::from_axis_angle(axis, a), 2e-4));
        }
    }

    #[test]
    fn rotations_keep_lengths() {
        for k in -100..100 {
            let a = k as f32 * 0.037;
            let (s, c) = unit(a);
            assert!((s * s + c * c - 1.0).abs() < 1e-6, "{a}");
            let m = Mat3::euler_zyx(Vec3::new(a, -a * 0.5, a * 2.0));
            assert!(((m * Vec3::new(1.0, 2.0, 3.0)).length() - 14f32.sqrt()).abs() < 1e-5);
        }
    }

    /// The tables a frame's pipelines look up (here and nowhere else) stay
    #[test]
    fn tables_stay_small_in_l1() {
        const L1_SHARE: usize = 8 * 1024;
        let tables = [
            (
                std::mem::size_of_val(&COARSE),
                std::ptr::from_ref(&COARSE) as usize,
            ),
            (
                std::mem::size_of_val(&FINE),
                std::ptr::from_ref(&FINE) as usize,
            ),
        ];
        assert!(tables.iter().map(|t| t.0).sum::<usize>() <= L1_SHARE);
        for (_, at) in tables {
            assert_eq!(at % 64, 0, "a table starts mid cache line");
        }
    }

    #[test]
    fn wraps_degrees() {
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(180.0), -180.0);
    }
}
