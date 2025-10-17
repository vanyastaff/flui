//! Extensions for egui integration
//!
//! This module provides conversion traits between flui_types and egui types.

use flui_types::Offset;

/// Extension trait for Offset to egui conversions
pub trait OffsetEguiExt {
    /// Convert to egui Pos2
    fn to_pos2(self) -> egui::Pos2;

    /// Convert to egui Vec2
    fn to_vec2(self) -> egui::Vec2;

    /// Create from egui Pos2
    fn from_pos2(pos: egui::Pos2) -> Self;

    /// Create from egui Vec2
    fn from_vec2(vec: egui::Vec2) -> Self;
}

impl OffsetEguiExt for Offset {
    fn to_pos2(self) -> egui::Pos2 {
        egui::pos2(self.dx, self.dy)
    }

    fn to_vec2(self) -> egui::Vec2 {
        egui::vec2(self.dx, self.dy)
    }

    fn from_pos2(pos: egui::Pos2) -> Self {
        Offset::new(pos.x, pos.y)
    }

    fn from_vec2(vec: egui::Vec2) -> Self {
        Offset::new(vec.x, vec.y)
    }
}

// Note: We cannot implement From traits due to orphan rules.
// Use the extension trait methods instead (to_pos2, to_vec2, from_pos2, from_vec2).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_egui_conversion() {
        let offset = Offset::new(10.0, 20.0);
        let pos2 = offset.to_pos2();
        let back = Offset::from_pos2(pos2);

        assert_eq!(offset, back);

        let vec2 = offset.to_vec2();
        let back2 = Offset::from_vec2(vec2);

        assert_eq!(offset, back2);
    }

    #[test]
    fn test_offset_from_egui() {
        let pos2 = egui::pos2(10.0, 20.0);
        let offset = Offset::from_pos2(pos2);
        assert_eq!(offset, Offset::new(10.0, 20.0));

        let vec2 = egui::vec2(10.0, 20.0);
        let offset = Offset::from_vec2(vec2);
        assert_eq!(offset, Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_offset_to_egui() {
        let offset = Offset::new(10.0, 20.0);
        let pos2 = offset.to_pos2();
        assert_eq!(pos2.x, 10.0);
        assert_eq!(pos2.y, 20.0);

        let vec2 = offset.to_vec2();
        assert_eq!(vec2.x, 10.0);
        assert_eq!(vec2.y, 20.0);
    }
}
