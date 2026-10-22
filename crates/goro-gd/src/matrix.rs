//! gd_matrix.c: affine matrices ([a, b, c, d, tx, ty]).

use crate::PointF;
use std::f64::consts::PI;

/// GD_EPSILON
pub const EPSILON: f64 = 1e-6;

/// gdAffineApplyToPointF()
pub fn apply_to_point_f(src: &PointF, affine: &[f64; 6]) -> PointF {
    let x = src.x;
    let y = src.y;
    PointF {
        x: x * affine[0] + y * affine[2] + affine[4],
        y: x * affine[1] + y * affine[3] + affine[5],
    }
}

/// gdAffineInvert(): None when the determinant is not positive
pub fn invert(src: &[f64; 6]) -> Option<[f64; 6]> {
    let mut r_det = src[0] * src[3] - src[1] * src[2];
    if r_det <= 0.0 {
        return None;
    }
    r_det = 1.0 / r_det;
    let mut dst = [0f64; 6];
    dst[0] = src[3] * r_det;
    dst[1] = -src[1] * r_det;
    dst[2] = -src[2] * r_det;
    dst[3] = src[0] * r_det;
    dst[4] = -src[4] * dst[0] - src[5] * dst[2];
    dst[5] = -src[4] * dst[1] - src[5] * dst[3];
    Some(dst)
}

/// gdAffineFlip()
pub fn flip(src: &[f64; 6], flip_h: bool, flip_v: bool) -> [f64; 6] {
    [
        if flip_h { -src[0] } else { src[0] },
        if flip_h { -src[1] } else { src[1] },
        if flip_v { -src[2] } else { src[2] },
        if flip_v { -src[3] } else { src[3] },
        if flip_h { -src[4] } else { src[4] },
        if flip_v { -src[5] } else { src[5] },
    ]
}

/// gdAffineConcat(): m1 then m2
pub fn concat(m1: &[f64; 6], m2: &[f64; 6]) -> [f64; 6] {
    [
        m1[0] * m2[0] + m1[1] * m2[2],
        m1[0] * m2[1] + m1[1] * m2[3],
        m1[2] * m2[0] + m1[3] * m2[2],
        m1[2] * m2[1] + m1[3] * m2[3],
        m1[4] * m2[0] + m1[5] * m2[2] + m2[4],
        m1[4] * m2[1] + m1[5] * m2[3] + m2[5],
    ]
}

/// gdAffineIdentity()
pub fn identity() -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

/// gdAffineScale()
pub fn scale(scale_x: f64, scale_y: f64) -> [f64; 6] {
    [scale_x, 0.0, 0.0, scale_y, 0.0, 0.0]
}

/// gdAffineRotate(): angle in degrees
pub fn rotate(angle: f64) -> [f64; 6] {
    let sin_t = (angle * PI / 180.0).sin();
    let cos_t = (angle * PI / 180.0).cos();
    [cos_t, sin_t, -sin_t, cos_t, 0.0, 0.0]
}

/// gdAffineShearHorizontal()
pub fn shear_horizontal(angle: f64) -> [f64; 6] {
    [1.0, 0.0, (angle * PI / 180.0).tan(), 1.0, 0.0, 0.0]
}

/// gdAffineShearVertical()
pub fn shear_vertical(angle: f64) -> [f64; 6] {
    [1.0, (angle * PI / 180.0).tan(), 0.0, 1.0, 0.0, 0.0]
}

/// gdAffineTranslate()
pub fn translate(offset_x: f64, offset_y: f64) -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, offset_x, offset_y]
}

/// gdAffineExpansion()
pub fn expansion(src: &[f64; 6]) -> f64 {
    (src[0] * src[3] - src[1] * src[2]).abs().sqrt()
}

/// gdAffineRectilinear()
pub fn rectilinear(m: &[f64; 6]) -> bool {
    (m[1].abs() < EPSILON && m[2].abs() < EPSILON) || (m[0].abs() < EPSILON && m[3].abs() < EPSILON)
}

/// gdAffineEqual()
pub fn equal(m1: &[f64; 6], m2: &[f64; 6]) -> bool {
    (0..6).all(|i| (m1[i] - m2[i]).abs() < EPSILON)
}
