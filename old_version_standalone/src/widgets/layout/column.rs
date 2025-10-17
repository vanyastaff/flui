//! Column widget - vertical layout container
//!
//! Similar to Flutter's Column widget. Arranges children vertically.

use crate::types::layout::{MainAxisAlignment, CrossAxisAlignment, MainAxisSize};

/// Column - arranges children vertically
///
/// Similar to Flutter's Column widget.
#[derive(Debug)]
pub struct Column {
    /// How to align children along the main axis (vertical)
    pub main_axis_alignment: MainAxisAlignment,

    /// How much space to occupy on main axis
    pub main_axis_size: MainAxisSize,

    /// How to align children along the cross axis (horizontal)
    pub cross_axis_alignment: CrossAxisAlignment,

    /// Spacing between children
    pub spacing: f32,

    /// Child widgets (using Any for now until we have proper widget system)
    pub children: Vec<Box<dyn std::any::Any>>,
}

impl Column {
    /// Create a new Column with default settings
    pub fn new() -> Self {
        Self {
            main_axis_alignment: MainAxisAlignment::Start,
            main_axis_size: MainAxisSize::Max,
            cross_axis_alignment: CrossAxisAlignment::Center,
            spacing: 0.0,
            children: Vec::new(),
        }
    }

    /// Set main axis alignment
    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Set main axis size
    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
        self
    }

    /// Set cross axis alignment
    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Set spacing between children
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Add a child widget
    pub fn add_child(mut self, child: Box<dyn std::any::Any>) -> Self {
        self.children.push(child);
        self
    }

    /// Set all children at once
    pub fn with_children(mut self, children: Vec<Box<dyn std::any::Any>>) -> Self {
        self.children = children;
        self
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

impl egui::Widget for Column {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = self.spacing;

            // Apply main axis alignment
            match self.main_axis_alignment {
                MainAxisAlignment::Start => {
                    // Default behavior
                }
                MainAxisAlignment::End => {
                    ui.add_space(ui.available_height());
                }
                MainAxisAlignment::Center => {
                    ui.add_space(ui.available_height() / 2.0);
                }
                MainAxisAlignment::SpaceBetween
                | MainAxisAlignment::SpaceAround
                | MainAxisAlignment::SpaceEvenly => {
                    // TODO: Implement proper spacing
                }
            }

            // Render children
            // Note: In real implementation, we would render actual widgets here
            // For now, just show placeholder
            ui.label(format!("Column with {} children", self.children.len()));
        })
        .response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_creation() {
        let column = Column::new();
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(column.main_axis_size, MainAxisSize::Max);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Center);
        assert_eq!(column.spacing, 0.0);
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_with_alignment() {
        let column = Column::new()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);

        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Start);
    }

    #[test]
    fn test_column_with_spacing() {
        let column = Column::new().with_spacing(16.0);
        assert_eq!(column.spacing, 16.0);
    }

    #[test]
    fn test_column_with_main_axis_size() {
        let column = Column::new().with_main_axis_size(MainAxisSize::Min);
        assert_eq!(column.main_axis_size, MainAxisSize::Min);
    }

    #[test]
    fn test_column_add_child() {
        let column = Column::new()
            .add_child(Box::new("child1"))
            .add_child(Box::new("child2"));

        assert_eq!(column.children.len(), 2);
    }

    #[test]
    fn test_column_with_children() {
        let children = vec![
            Box::new("child1") as Box<dyn std::any::Any>,
            Box::new("child2") as Box<dyn std::any::Any>,
            Box::new("child3") as Box<dyn std::any::Any>,
        ];

        let column = Column::new().with_children(children);
        assert_eq!(column.children.len(), 3);
    }
}
