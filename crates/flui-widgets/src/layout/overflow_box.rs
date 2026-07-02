//! [`OverflowBox`] — lays child out under modified constraints.

use flui_objects::{OverflowBoxFit, RenderConstrainedOverflowBox};
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, Pixels};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Lays its child out as if it lived in a box with different constraints,
/// potentially allowing the child to overflow the parent's available space.
///
/// Each per-axis constraint (`min_width`, `max_width`, `min_height`,
/// `max_height`) overrides the corresponding incoming value when `Some`;
/// unset axes inherit the parent's value.  The `fit` field controls how
/// this widget reports its own size back to its parent.
///
/// Flutter parity: `widgets/basic.dart` `OverflowBox` over
/// [`RenderConstrainedOverflowBox`].
#[derive(Clone, Debug)]
pub struct OverflowBox {
    alignment: Alignment,
    min_width: Option<Pixels>,
    max_width: Option<Pixels>,
    min_height: Option<Pixels>,
    max_height: Option<Pixels>,
    fit: OverflowBoxFit,
    child: Child,
}

impl OverflowBox {
    /// Creates the widget with center alignment, no constraint overrides, and
    /// `OverflowBoxFit::Max` (claims all available parent space).
    pub fn new() -> Self {
        Self {
            alignment: Alignment::CENTER,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            fit: OverflowBoxFit::Max,
            child: Child::empty(),
        }
    }

    /// Sets the alignment of the child within the overflow box.
    #[must_use]
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Overrides the minimum width constraint passed to the child.
    #[must_use]
    pub fn with_min_width(mut self, min_width: Pixels) -> Self {
        self.min_width = Some(min_width);
        self
    }

    /// Overrides the maximum width constraint passed to the child.
    #[must_use]
    pub fn with_max_width(mut self, max_width: Pixels) -> Self {
        self.max_width = Some(max_width);
        self
    }

    /// Overrides the minimum height constraint passed to the child.
    #[must_use]
    pub fn with_min_height(mut self, min_height: Pixels) -> Self {
        self.min_height = Some(min_height);
        self
    }

    /// Overrides the maximum height constraint passed to the child.
    #[must_use]
    pub fn with_max_height(mut self, max_height: Pixels) -> Self {
        self.max_height = Some(max_height);
        self
    }

    /// Sets the fit policy (how this widget reports its own size).
    #[must_use]
    pub fn with_fit(mut self, fit: OverflowBoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl Default for OverflowBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView for OverflowBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderConstrainedOverflowBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderConstrainedOverflowBox::new(
            self.alignment,
            self.min_width,
            self.max_width,
            self.min_height,
            self.max_height,
            self.fit,
        )
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_alignment(self.alignment);
        render_object.set_min_width(self.min_width);
        render_object.set_max_width(self.max_width);
        render_object.set_min_height(self.min_height);
        render_object.set_max_height(self.max_height);
        render_object.set_fit(self.fit);
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(OverflowBox);
