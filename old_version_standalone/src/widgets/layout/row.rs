//! Row layout widget - horizontal arrangement of children
//!
//! Similar to Flutter's Row widget.

use crate::types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use crate::widgets::{NebulaWidget, RenderObjectWidget};
use egui::Widget;

/// A widget that displays its children in a horizontal array.
///
/// Similar to Flutter's `Row`.
///
/// # Example
///
/// ```rust,ignore
/// use nebula_ui::widgets::layout::Row;
/// use nebula_ui::widgets::primitives::Text;
///
/// // Use egui's horizontal layout directly for now
/// ui.horizontal(|ui| {
///     ui.label("First");
///     ui.label("Second");
///     ui.label("Third");
/// });
/// ```
#[derive(Debug)]
pub struct Row {
    /// How the children should be placed along the main axis
    pub main_axis_alignment: MainAxisAlignment,

    /// How much space should be occupied in the main axis
    pub main_axis_size: MainAxisSize,

    /// How the children should be placed along the cross axis
    pub cross_axis_alignment: CrossAxisAlignment,

    /// Spacing between children
    pub spacing: f32,
}

impl Row {
    /// Create a new row.
    pub fn new() -> Self {
        Self {
            main_axis_alignment: MainAxisAlignment::Start,
            main_axis_size: MainAxisSize::Max,
            cross_axis_alignment: CrossAxisAlignment::Center,
            spacing: 0.0,
        }
    }


    /// Set the main axis alignment.
    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Set the main axis size.
    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
        self
    }

    /// Set the cross axis alignment.
    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Set the spacing between children.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

impl NebulaWidget for Row {}

impl RenderObjectWidget for Row {}

impl Widget for Row {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Row is a layout helper - actual children are added via the closure pattern
        // This is just a configuration holder
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = self.spacing;
            ui.label("Row (use horizontal closure instead)")
        })
        .response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_creation() {
        let row = Row::new();
        assert_eq!(row.spacing, 0.0);
        assert_eq!(row.main_axis_alignment, MainAxisAlignment::Start);
    }

    #[test]
    fn test_row_with_spacing() {
        let row = Row::new().with_spacing(10.0);
        assert_eq!(row.spacing, 10.0);
    }

    #[test]
    fn test_row_with_alignment() {
        let row = Row::new()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);

        assert_eq!(row.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(row.cross_axis_alignment, CrossAxisAlignment::Start);
    }
}
