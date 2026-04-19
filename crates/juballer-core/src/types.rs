use serde::{Deserialize, Serialize};

/// Pixel rectangle. Origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }
    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// 8-bit-per-channel RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Color {
    pub const BLACK: Color = Color(0, 0, 0, 0xff);
    pub const WHITE: Color = Color(0xff, 0xff, 0xff, 0xff);
    pub const TRANSPARENT: Color = Color(0, 0, 0, 0);

    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 0xff)
    }
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(r, g, b, a)
    }

    pub fn as_linear_f32(self) -> [f32; 4] {
        fn srgb_to_linear(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            srgb_to_linear(self.0),
            srgb_to_linear(self.1),
            srgb_to_linear(self.2),
            self.3 as f32 / 255.0,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
        assert_eq!(r.area(), 5000);
        assert!(!r.is_empty());
    }

    #[test]
    fn rect_zero_is_empty() {
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::new(0, 0, 0, 5).is_empty());
        assert!(Rect::new(0, 0, 5, 0).is_empty());
    }

    #[test]
    fn color_constructors() {
        assert_eq!(Color::rgb(1, 2, 3), Color(1, 2, 3, 0xff));
        assert_eq!(Color::rgba(1, 2, 3, 4), Color(1, 2, 3, 4));
        assert_eq!(Color::BLACK, Color(0, 0, 0, 0xff));
    }

    #[test]
    fn color_linear_white_roundtrip() {
        let l = Color::WHITE.as_linear_f32();
        assert!((l[0] - 1.0).abs() < 1e-6);
        assert!((l[3] - 1.0).abs() < 1e-6);
    }
}
