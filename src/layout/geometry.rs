#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub(super) fn union(self, other: Self) -> Self {
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
