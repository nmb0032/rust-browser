#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Unclipped device-pixel rectangle; clipping happens at rasterization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActualRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn actual(self, scale: f32) -> Option<ActualRect> {
        if !scale.is_finite()
            || scale <= 0.0
            || ![self.x, self.y, self.width, self.height]
                .into_iter()
                .all(f32::is_finite)
        {
            return None;
        }
        let edges = [
            self.x * scale,
            self.y * scale,
            (self.x + self.width) * scale,
            (self.y + self.height) * scale,
        ];
        edges.into_iter().all(f32::is_finite).then(|| ActualRect {
            left: edges[0].floor() as i32,
            top: edges[1].floor() as i32,
            right: edges[2].ceil() as i32,
            bottom: edges[3].ceil() as i32,
        })
    }

    pub(crate) fn union(self, other: Self) -> Self {
        if self.width == 0.0 && self.height == 0.0 {
            return other;
        }
        if other.width == 0.0 && other.height == 0.0 {
            return self;
        }
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        Self {
            x: left,
            y: top,
            width: (self.x + self.width).max(other.x + other.width) - left,
            height: (self.y + self.height).max(other.y + other.height) - top,
        }
    }
}
