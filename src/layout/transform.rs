//! Column-vector CSS matrices and finite plane projection. Layout coordinates
//! remain unchanged; raster and input share the same inverse homography.
use super::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    pub fn then(self, rhs: Self) -> Self {
        let mut out = [[0.0; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, value) in row.iter_mut().enumerate() {
                *value = (0..4).map(|k| self.0[r][k] * rhs.0[k][c]).sum();
            }
        }
        Self(out)
    }
    pub fn translation(x: f64, y: f64, z: f64) -> Self {
        let mut m = Self::IDENTITY;
        m.0[0][3] = x;
        m.0[1][3] = y;
        m.0[2][3] = z;
        m
    }
    pub fn scale(x: f64, y: f64, z: f64) -> Self {
        let mut m = Self::IDENTITY;
        m.0[0][0] = x;
        m.0[1][1] = y;
        m.0[2][2] = z;
        m
    }
    pub fn rotation(axis: [f64; 3], angle: f64) -> Self {
        let norm = axis.iter().map(|a| a * a).sum::<f64>().sqrt();
        if norm == 0.0 {
            return Self::IDENTITY;
        }
        let [x, y, z] = axis.map(|a| a / norm);
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        Self([
            [t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0],
            [t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0],
            [t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }
    pub fn perspective(distance: f64) -> Self {
        let mut m = Self::IDENTITY;
        m.0[3][2] = -1.0 / distance.max(1.0);
        m
    }
    pub fn around(self, x: f64, y: f64, z: f64) -> Self {
        Self::translation(x, y, z)
            .then(self)
            .then(Self::translation(-x, -y, -z))
    }
    pub fn finite(self) -> bool {
        self.0.iter().flatten().all(|a| a.is_finite())
    }
    pub fn homogeneous(self, x: f64, y: f64) -> [f64; 4] {
        self.0.map(|r| r[0] * x + r[1] * y + r[3])
    }
    pub fn project(self, x: f64, y: f64) -> Option<(f32, f32)> {
        let [x, y, _, w] = self.homogeneous(x, y);
        if w < 1e-6 || !w.is_finite() {
            return None;
        }
        let (x, y) = ((x / w) as f32, (y / w) as f32);
        (x.is_finite() && y.is_finite()).then_some((x, y))
    }
    /// Invert the z=0 plane, not an arbitrary z=0 point in the inverse 4D
    /// matrix. Recheck W so points behind the eye cannot be hit or sampled.
    pub fn unproject(self, x: f32, y: f32) -> Option<(f32, f32)> {
        let a = [
            [self.0[0][0], self.0[0][1], self.0[0][3]],
            [self.0[1][0], self.0[1][1], self.0[1][3]],
            [self.0[3][0], self.0[3][1], self.0[3][3]],
        ];
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let [[a, b, c], [d, e, f], [g, h, i]] = a;
        let (x, y) = (x as f64, y as f64);
        let q = [
            (e * i - f * h) * x + (c * h - b * i) * y + b * f - c * e,
            (f * g - d * i) * x + (a * i - c * g) * y + c * d - a * f,
            (d * h - e * g) * x + (b * g - a * h) * y + a * e - b * d,
        ];
        if q[2].abs() < 1e-12 {
            return None;
        }
        let (u, v) = ((q[0] / q[2]) as f32, (q[1] / q[2]) as f32);
        if !u.is_finite() || !v.is_finite() || self.project(u as f64, v as f64).is_none() {
            return None;
        }
        Some((u, v))
    }
    /// Clip a quad in homogeneous space before division by W. Merely
    /// dividing four corners fails when a perspective edge crosses the eye.
    pub fn bounds(self, rect: Rect) -> Option<Rect> {
        if rect.width <= 0.0 || rect.height <= 0.0 || !self.finite() {
            return None;
        }
        let mut quad = Vec::new();
        for (x, y) in [
            (rect.x, rect.y),
            (rect.x + rect.width, rect.y),
            (rect.x + rect.width, rect.y + rect.height),
            (rect.x, rect.y + rect.height),
        ] {
            quad.push(self.homogeneous(x as f64, y as f64));
        }
        let mut clipped = Vec::new();
        for i in 0..4 {
            let a = quad[i];
            let b = quad[(i + 1) % 4];
            if a[3] >= 1e-6 {
                clipped.push(a);
            }
            if (a[3] >= 1e-6) != (b[3] >= 1e-6) {
                let t = (1e-6 - a[3]) / (b[3] - a[3]);
                clipped.push(std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t));
            }
        }
        let mut bounds: Option<(f64, f64, f64, f64)> = None;
        for p in clipped {
            let x = (p[0] / p[3]).clamp(-1e7, 1e7);
            let y = (p[1] / p[3]).clamp(-1e7, 1e7);
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            bounds = Some(bounds.map_or((x, y, x, y), |(l, t, r, b)| {
                (l.min(x), t.min(y), r.max(x), b.max(y))
            }));
        }
        bounds.map(|(l, t, r, b)| Rect {
            x: l as f32,
            y: t as f32,
            width: (r - l) as f32,
            height: (b - t) as f32,
        })
    }
    pub fn back_facing(self) -> bool {
        let a = self.project(0.0, 0.0);
        let b = self.project(1.0, 0.0);
        let c = self.project(0.0, 1.0);
        match (a, b, c) {
            (Some(a), Some(b), Some(c)) => {
                (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0) < 0.0
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order_inverse_perspective_and_singular_planes() {
        let m = Mat4::translation(10.0, 20.0, 0.0).then(Mat4::scale(2.0, 3.0, 1.0));
        assert_eq!(m.project(5.0, 6.0), Some((20.0, 38.0)));
        assert_eq!(m.unproject(20.0, 38.0), Some((5.0, 6.0)));
        let m = Mat4::perspective(100.0).then(Mat4::translation(0.0, 0.0, 50.0));
        assert_eq!(m.project(10.0, 20.0), Some((20.0, 40.0)));
        assert_eq!(m.unproject(20.0, 40.0), Some((10.0, 20.0)));
        assert!(Mat4::scale(0.0, 1.0, 1.0).unproject(0.0, 0.0).is_none());
        assert!(
            Mat4::perspective(10.0)
                .then(Mat4::translation(0.0, 0.0, 20.0))
                .project(0.0, 0.0)
                .is_none()
        );
        assert!(Mat4::rotation([0.0, 1.0, 0.0], std::f64::consts::PI).back_facing());
    }
}
