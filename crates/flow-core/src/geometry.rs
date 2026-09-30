use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

/// A point or vector in world or canvas-local screen coordinates.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub const ZERO: Self = Self::new(0.0, 0.0);
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn distance(self, other: Self) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
    /// Snap to a positive grid interval. Invalid intervals leave the point unchanged.
    pub fn snapped(self, grid: f32) -> Self {
        if !grid.is_finite() || grid <= 0.0 {
            return self;
        }
        Self::new(
            (self.x / grid).round() * grid,
            (self.y / grid).round() * grid,
        )
    }
}
impl Add for Point {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }
}
impl Sub for Point {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y)
    }
}
impl Mul<f32> for Point {
    type Output = Self;
    fn mul(self, b: f32) -> Self {
        Self::new(self.x * b, self.y * b)
    }
}
impl Div<f32> for Point {
    type Output = Self;
    fn div(self, b: f32) -> Self {
        Self::new(self.x / b, self.y / b)
    }
}
impl AddAssign for Point {
    fn add_assign(&mut self, b: Self) {
        *self = *self + b;
    }
}
impl SubAssign for Point {
    fn sub_assign(&mut self, b: Self) {
        *self = *self - b;
    }
}

/// Non-negative rectangular extent. Graph nodes require strictly positive sizes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
    pub fn is_finite(self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }
    pub fn is_positive(self) -> bool {
        self.is_finite() && self.width > 0.0 && self.height > 0.0
    }
}

/// Axis-aligned rectangle. Use `from_points` for a drag in either direction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}
impl Rect {
    pub const fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }
    pub fn from_points(a: Point, b: Point) -> Self {
        Self::new(
            Point::new(a.x.min(b.x), a.y.min(b.y)),
            Size::new((a.x - b.x).abs(), (a.y - b.y).abs()),
        )
    }
    pub fn min_x(self) -> f32 {
        self.origin.x
    }
    pub fn min_y(self) -> f32 {
        self.origin.y
    }
    pub fn max_x(self) -> f32 {
        self.origin.x + self.size.width
    }
    pub fn max_y(self) -> f32 {
        self.origin.y + self.size.height
    }
    pub fn center(self) -> Point {
        self.origin + Point::new(self.size.width * 0.5, self.size.height * 0.5)
    }
    pub fn is_valid(self) -> bool {
        self.origin.is_finite()
            && self.size.is_finite()
            && self.size.width >= 0.0
            && self.size.height >= 0.0
            && self.max_x().is_finite()
            && self.max_y().is_finite()
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.min_x() && p.x <= self.max_x() && p.y >= self.min_y() && p.y <= self.max_y()
    }
    pub fn contains_rect(self, r: Self) -> bool {
        self.contains(r.origin) && self.contains(Point::new(r.max_x(), r.max_y()))
    }
    pub fn intersects(self, r: Self) -> bool {
        self.min_x() <= r.max_x()
            && self.max_x() >= r.min_x()
            && self.min_y() <= r.max_y()
            && self.max_y() >= r.min_y()
    }
    pub fn union(self, r: Self) -> Self {
        Self::from_points(
            Point::new(self.min_x().min(r.min_x()), self.min_y().min(r.min_y())),
            Point::new(self.max_x().max(r.max_x()), self.max_y().max(r.max_y())),
        )
    }
    pub fn expanded(self, padding: f32) -> Self {
        let p = if padding.is_finite() {
            padding.max(-0.5 * self.size.width.min(self.size.height))
        } else {
            0.0
        };
        Self::new(
            self.origin - Point::new(p, p),
            Size::new(self.size.width + 2.0 * p, self.size.height + 2.0 * p),
        )
    }
    pub fn translated(self, delta: Point) -> Self {
        Self::new(self.origin + delta, self.size)
    }
}

/// Pan is measured in screen pixels, zoom is screen pixels per world unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Viewport {
    pub pan: Point,
    pub zoom: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
}
impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: Point::ZERO,
            zoom: 1.0,
            min_zoom: 0.1,
            max_zoom: 3.0,
        }
    }
}
impl Viewport {
    pub fn is_valid(self) -> bool {
        self.pan.is_finite()
            && self.zoom.is_finite()
            && self.min_zoom.is_finite()
            && self.max_zoom.is_finite()
            && self.min_zoom > 0.0
            && self.max_zoom >= self.min_zoom
            && self.zoom >= self.min_zoom
            && self.zoom <= self.max_zoom
    }
    pub fn world_to_screen(self, world: Point) -> Point {
        world * self.zoom + self.pan
    }
    pub fn screen_to_world(self, screen: Point) -> Point {
        (screen - self.pan) / self.zoom
    }
    pub fn world_rect_to_screen(self, r: Rect) -> Rect {
        Rect::new(
            self.world_to_screen(r.origin),
            Size::new(r.size.width * self.zoom, r.size.height * self.zoom),
        )
    }
    pub fn visible_world_rect(self, canvas: Size) -> Rect {
        Rect::new(
            self.screen_to_world(Point::ZERO),
            Size::new(canvas.width / self.zoom, canvas.height / self.zoom),
        )
    }
    /// Set the zoom, keeping the world point under `screen_anchor` fixed.
    /// Returns false for invalid input; valid scales are clamped to zoom limits.
    pub fn zoom_at(&mut self, screen_anchor: Point, zoom: f32) -> bool {
        if !self.is_valid() || !screen_anchor.is_finite() || !zoom.is_finite() || zoom <= 0.0 {
            return false;
        }
        let world = self.screen_to_world(screen_anchor);
        let next_zoom = zoom.clamp(self.min_zoom, self.max_zoom);
        let next_pan = screen_anchor - world * next_zoom;
        if !next_pan.is_finite() {
            return false;
        }
        self.zoom = next_zoom;
        self.pan = next_pan;
        true
    }
    pub fn pan_by(&mut self, delta: Point) -> bool {
        let pan = self.pan + delta;
        if !delta.is_finite() || !pan.is_finite() {
            return false;
        }
        self.pan = pan;
        true
    }
    /// Center content with screen-pixel padding. Clamped zoom may make extremely
    /// large content overflow; this respects the application's configured limits.
    pub fn fit(&mut self, bounds: Rect, canvas: Size, padding: f32) -> bool {
        if !self.is_valid()
            || !bounds.is_valid()
            || !canvas.is_positive()
            || !padding.is_finite()
            || padding < 0.0
        {
            return false;
        }
        let w = canvas.width - 2.0 * padding;
        let h = canvas.height - 2.0 * padding;
        if w <= 0.0 || h <= 0.0 {
            return false;
        }
        let zoom = (w / bounds.size.width.max(1.0))
            .min(h / bounds.size.height.max(1.0))
            .clamp(self.min_zoom, self.max_zoom);
        let pan = Point::new(canvas.width * 0.5, canvas.height * 0.5) - bounds.center() * zoom;
        if !pan.is_finite() {
            return false;
        }
        self.zoom = zoom;
        self.pan = pan;
        true
    }
}

/// Maps graph bounds into a minimap rectangle, preserving aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapTransform {
    pub world_bounds: Rect,
    pub map_bounds: Rect,
    pub scale: f32,
    pub offset: Point,
}
impl MinimapTransform {
    pub fn new(world_bounds: Rect, map_bounds: Rect, padding: f32) -> Option<Self> {
        if !world_bounds.is_valid()
            || !map_bounds.is_valid()
            || !padding.is_finite()
            || padding < 0.0
        {
            return None;
        }
        let inner = map_bounds.expanded(-padding);
        if !inner.size.is_positive() {
            return None;
        }
        let scale = (inner.size.width / world_bounds.size.width.max(1.0))
            .min(inner.size.height / world_bounds.size.height.max(1.0));
        let offset = inner.center() - world_bounds.center() * scale;
        if !scale.is_finite() || scale <= 0.0 || !offset.is_finite() {
            return None;
        }
        Some(Self {
            world_bounds,
            map_bounds,
            scale,
            offset,
        })
    }
    pub fn world_to_map(self, p: Point) -> Point {
        p * self.scale + self.offset
    }
    pub fn map_to_world(self, p: Point) -> Point {
        (p - self.offset) / self.scale
    }
    pub fn world_rect_to_map(self, r: Rect) -> Rect {
        Rect::new(
            self.world_to_map(r.origin),
            Size::new(r.size.width * self.scale, r.size.height * self.scale),
        )
    }
}
