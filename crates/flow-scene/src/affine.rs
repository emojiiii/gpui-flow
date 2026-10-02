use crate::{Bounds, Point};
use serde::{Deserialize, Serialize};
use std::ops::Mul;

/// Column-vector affine transform: `(a*x+c*y+tx, b*x+d*y+ty)`.
/// `parent * local` applies the local transform first, then its parent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}
impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl Affine {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };
    pub const fn new(a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) -> Self {
        Self { a, b, c, d, tx, ty }
    }
    pub const fn translation(x: f32, y: f32) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }
    pub const fn scale(x: f32, y: f32) -> Self {
        Self {
            a: x,
            d: y,
            b: 0.0,
            c: 0.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
    pub fn rotation(degrees: f32) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Self::new(c, s, -s, c, 0.0, 0.0)
    }
    pub fn skew(x_degrees: f32, y_degrees: f32) -> Self {
        Self::new(
            1.0,
            y_degrees.to_radians().tan(),
            x_degrees.to_radians().tan(),
            1.0,
            0.0,
            0.0,
        )
    }
    pub fn around(self, origin: Point) -> Self {
        Self::translation(origin.x, origin.y) * self * Self::translation(-origin.x, -origin.y)
    }
    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.tx, self.ty]
            .iter()
            .all(|v| v.is_finite())
    }
    /// A finite axis-aligned positive uniform scale plus translation. Rotation,
    /// reflection and shear do not meet this exact geometric predicate.
    pub fn is_positive_uniform_scale_translation(self) -> bool {
        self.is_finite() && self.b == 0. && self.c == 0. && self.a > 0. && self.a == self.d
    }
    pub fn determinant(self) -> f64 {
        self.a as f64 * self.d as f64 - self.b as f64 * self.c as f64
    }
    pub fn inverse(self) -> Option<Self> {
        if !self.is_finite() {
            return None;
        }
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let a = self.d as f64 / det;
        let b = -(self.b as f64) / det;
        let c = -(self.c as f64) / det;
        let d = self.a as f64 / det;
        let tx = -(a * self.tx as f64 + c * self.ty as f64);
        let ty = -(b * self.tx as f64 + d * self.ty as f64);
        let result = Self::new(a as f32, b as f32, c as f32, d as f32, tx as f32, ty as f32);
        result.is_finite().then_some(result)
    }
    pub fn transform_point(self, p: Point) -> Point {
        Point::new(
            (self.a as f64 * p.x as f64 + self.c as f64 * p.y as f64 + self.tx as f64) as f32,
            (self.b as f64 * p.x as f64 + self.d as f64 * p.y as f64 + self.ty as f64) as f32,
        )
    }
    pub fn transform_vector(self, p: Point) -> Point {
        Point::new(
            (self.a as f64 * p.x as f64 + self.c as f64 * p.y as f64) as f32,
            (self.b as f64 * p.x as f64 + self.d as f64 * p.y as f64) as f32,
        )
    }
    pub fn transform_bounds(self, r: Bounds) -> Bounds {
        let corners = [
            r.origin,
            Point::new(r.max_x(), r.min_y()),
            Point::new(r.max_x(), r.max_y()),
            Point::new(r.min_x(), r.max_y()),
        ];
        crate::geometry::point_bounds(corners.into_iter().map(|p| self.transform_point(p)))
            .unwrap_or_default()
    }
    /// Largest singular value: conservative conversion of local distances.
    pub fn max_scale(self) -> f32 {
        let a = self.a as f64;
        let b = self.b as f64;
        let c = self.c as f64;
        let d = self.d as f64;
        let sum = a * a + b * b + c * c + d * d;
        let det = a * d - b * c;
        ((sum + (sum * sum - 4.0 * det * det).max(0.0).sqrt()) * 0.5).sqrt() as f32
    }
}
impl Mul for Affine {
    type Output = Self;
    fn mul(self, r: Self) -> Self {
        Self::new(
            (self.a as f64 * r.a as f64 + self.c as f64 * r.b as f64) as f32,
            (self.b as f64 * r.a as f64 + self.d as f64 * r.b as f64) as f32,
            (self.a as f64 * r.c as f64 + self.c as f64 * r.d as f64) as f32,
            (self.b as f64 * r.c as f64 + self.d as f64 * r.d as f64) as f32,
            (self.a as f64 * r.tx as f64 + self.c as f64 * r.ty as f64 + self.tx as f64) as f32,
            (self.b as f64 * r.tx as f64 + self.d as f64 * r.ty as f64 + self.ty as f64) as f32,
        )
    }
}
