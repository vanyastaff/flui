//! RenderConstrainedBox - applies additional constraints to child
//!
//! This is the render object for ConstrainedBox and SizedBox widgets.
//! Similar to Flutter's RenderConstrainedBox.
//!
//! It takes incoming constraints and combines them with its own additional
//! constraints before passing to the child.

use crate::{BoxConstraints, Offset, RenderObject, Size};

/// RenderConstrainedBox - applies additional constraints to child
///
/// Takes the incoming constraints and tightens/loosens them with additional
/// constraints before laying out the child. This is used for:
/// - SizedBox (tight constraints to specific size)
/// - ConstrainedBox (additional min/max constraints)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderConstrainedBox, BoxConstraints};
/// use flui_types::Size;
///
/// // Create a box that forces size to 100x100
/// let mut constrained = RenderConstrainedBox::new(
///     BoxConstraints::tight(Size::new(100.0, 100.0))
/// );
/// ```
#[derive(Debug)]
pub struct RenderConstrainedBox {
    /// Additional constraints to apply
    additional_constraints: BoxConstraints,

    /// The single child
    child: Option<Box<dyn RenderObject>>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Layout dirty flag
    needs_layout_flag: bool,

    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderConstrainedBox {
    /// Create a new RenderConstrainedBox with additional constraints
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self {
            additional_constraints,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create a RenderConstrainedBox with tight constraints (for SizedBox)
    pub fn tight(size: Size) -> Self {
        Self::new(BoxConstraints::tight(size))
    }

    /// Create a RenderConstrainedBox with loose constraints
    pub fn loose(size: Size) -> Self {
        Self::new(BoxConstraints::loose(size))
    }

    /// Set additional constraints
    pub fn set_additional_constraints(&mut self, constraints: BoxConstraints) {
        if self.additional_constraints != constraints {
            self.additional_constraints = constraints;
            self.mark_needs_layout();
        }
    }

    /// Get additional constraints
    pub fn additional_constraints(&self) -> BoxConstraints {
        self.additional_constraints
    }

    /// Set the child
    pub fn set_child(&mut self, child: Box<dyn RenderObject>) {
        self.child = Some(child);
        self.mark_needs_layout();
    }

    /// Remove the child
    pub fn remove_child(&mut self) {
        self.child = None;
        self.mark_needs_layout();
    }

    /// Get reference to child
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_deref()
    }

    /// Perform layout
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);

        if let Some(child) = &mut self.child {
            // Enforce additional constraints on top of incoming constraints
            let child_constraints = constraints.enforce(self.additional_constraints);
            self.size = child.layout(child_constraints);
        } else {
            // No child - use smallest size that satisfies both constraints
            self.size = constraints.enforce(self.additional_constraints).smallest();
        }

        self.size
    }
}

impl Default for RenderConstrainedBox {
    fn default() -> Self {
        Self::new(BoxConstraints::default())
    }
}

impl RenderObject for RenderConstrainedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.needs_layout_flag = false;
        self.perform_layout(constraints)
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderBox;

    #[test]
    fn test_render_constrained_box_new() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let constrained = RenderConstrainedBox::new(constraints);
        assert!(constrained.needs_layout());
        assert_eq!(constrained.additional_constraints(), constraints);
    }

    #[test]
    fn test_render_constrained_box_tight() {
        let constrained = RenderConstrainedBox::tight(Size::new(100.0, 50.0));
        let expected = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(constrained.additional_constraints(), expected);
    }

    #[test]
    fn test_render_constrained_box_loose() {
        let constrained = RenderConstrainedBox::loose(Size::new(100.0, 50.0));
        let expected = BoxConstraints::loose(Size::new(100.0, 50.0));
        assert_eq!(constrained.additional_constraints(), expected);
    }

    #[test]
    fn test_render_constrained_box_no_child() {
        let mut constrained = RenderConstrainedBox::tight(Size::new(100.0, 50.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = constrained.layout(constraints);

        // No child - should use tight constraints (100x50)
        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_render_constrained_box_with_child() {
        let mut constrained = RenderConstrainedBox::tight(Size::new(100.0, 50.0));
        constrained.set_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = constrained.layout(constraints);

        // Child receives tight constraints (100x50)
        assert_eq!(size, Size::new(100.0, 50.0));
        assert_eq!(constrained.child().unwrap().size(), Size::new(100.0, 50.0));
    }

    #[test]
    fn test_render_constrained_box_enforce_min() {
        // Additional constraints: min 50x50, max 150x150
        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let mut constrained = RenderConstrainedBox::new(additional);
        constrained.set_child(Box::new(RenderBox::new()));

        // Incoming: 0-200, 0-200
        // Enforced: max(0,50)-min(200,150), max(0,50)-min(200,150) = 50-150, 50-150
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = constrained.layout(constraints);

        // Child gets 50-150 constraints, RenderBox will choose max (150x150)
        assert_eq!(size, Size::new(150.0, 150.0));
    }

    #[test]
    fn test_render_constrained_box_set_additional_constraints() {
        let mut constrained = RenderConstrainedBox::tight(Size::new(100.0, 50.0));

        let new_constraints = BoxConstraints::tight(Size::new(200.0, 100.0));
        constrained.set_additional_constraints(new_constraints);

        assert_eq!(constrained.additional_constraints(), new_constraints);
        assert!(constrained.needs_layout());
    }

    #[test]
    fn test_render_constrained_box_remove_child() {
        let mut constrained = RenderConstrainedBox::tight(Size::new(100.0, 50.0));
        constrained.set_child(Box::new(RenderBox::new()));

        assert!(constrained.child().is_some());

        constrained.remove_child();
        assert!(constrained.child().is_none());
        assert!(constrained.needs_layout());
    }

    #[test]
    fn test_render_constrained_box_visit_children() {
        let mut constrained = RenderConstrainedBox::new(BoxConstraints::default());
        constrained.set_child(Box::new(RenderBox::new()));

        let mut count = 0;
        constrained.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_constrained_box_visit_children_no_child() {
        let constrained = RenderConstrainedBox::new(BoxConstraints::default());

        let mut count = 0;
        constrained.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }
}
