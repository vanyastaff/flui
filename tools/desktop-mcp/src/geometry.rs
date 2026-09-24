//! Screen rectangles in physical pixels.

use std::fmt;

use serde::Serialize;

/// An axis-aligned rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Rect {
    /// From left/top/right/bottom edges; an inverted rect is empty.
    #[cfg(any(target_os = "windows", test))]
    pub fn from_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            x: left,
            y: top,
            width: right.saturating_sub(left).max(0) as u32,
            height: bottom.saturating_sub(top).max(0) as u32,
        }
    }

    /// The centre point.
    #[cfg(any(target_os = "windows", test))]
    pub fn center(&self) -> (i32, i32) {
        (
            self.x + (self.width / 2) as i32,
            self.y + (self.height / 2) as i32,
        )
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} at ({}, {})",
            self.width, self.height, self.x, self.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_rounds_down() {
        let r = Rect::from_ltrb(10, 20, 15, 30);
        assert_eq!(r.center(), (12, 25));
    }

    #[test]
    fn inverted_rect_is_empty() {
        let r = Rect::from_ltrb(5, 5, 0, 0);
        assert_eq!((r.width, r.height), (0, 0));
    }
}
