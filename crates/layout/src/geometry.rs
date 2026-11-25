//! Minimal backend-neutral geometry. Deliberately not `mkui-wgpu::types::Rect`
//! — this crate must not depend on any renderer crate (see the crate-level
//! docs' "No renderer types" rule), so it keeps its own small `f32` shapes. A
//! consuming backend converts these into its own geometry at the call site.

/// A 2D point in logical pixels, origin top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, rhs: Point) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

/// A 2D size in logical pixels. Never negative in practice — callers clamp
/// at the measurement/arithmetic boundary rather than carrying signed sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned box: `origin` is the top-left corner, `size` extends
/// right/down. This is the **border box** — padding and borders are already
/// folded into it, matching every other coordinate this crate hands out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    pub fn right(&self) -> f32 {
        self.origin.x + self.size.width
    }

    pub fn bottom(&self) -> f32 {
        self.origin.y + self.size.height
    }
}
