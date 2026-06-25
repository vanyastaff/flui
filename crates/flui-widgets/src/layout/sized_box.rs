//! [`SizedBox`] — forces a specific size on its child (or itself).

use flui_objects::RenderConstrainedBox;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::px;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// A box with a specific size that forces its child to that size.
///
/// Flutter parity: `widgets/basic.dart` `SizedBox`. Like Flutter, this is a
/// `RenderConstrainedBox` whose additional constraints are tight for the given
/// width/height; an unset dimension passes the parent's constraint through on
/// that axis (`BoxConstraints.tightFor`).
#[derive(Clone, Debug, Default)]
pub struct SizedBox {
    width: Option<f32>,
    height: Option<f32>,
    child: Child,
}

impl SizedBox {
    /// A box that forces both `width` and `height` on its child.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
            child: Child::empty(),
        }
    }

    /// A square box of the given side length.
    pub fn square(dimension: f32) -> Self {
        Self::new(dimension, dimension)
    }

    /// A box that forces only its width; height passes through.
    pub fn width(width: f32) -> Self {
        Self {
            width: Some(width),
            height: None,
            child: Child::empty(),
        }
    }

    /// A box that forces only its height; width passes through.
    pub fn height(height: f32) -> Self {
        Self {
            width: None,
            height: Some(height),
            child: Child::empty(),
        }
    }

    /// A box that becomes as large as its parent allows (infinite tight on
    /// both axes — Flutter's `SizedBox.expand`).
    pub fn expand() -> Self {
        Self::new(f32::INFINITY, f32::INFINITY)
    }

    /// A box that becomes as small as its parent allows (zero on both axes —
    /// Flutter's `SizedBox.shrink`).
    pub fn shrink() -> Self {
        Self::new(0.0, 0.0)
    }

    /// Set the sized child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Build `BoxConstraints.tightFor(width, height)`: tight where a dimension
    /// is set, pass-through (`0..=∞`) where it is not.
    fn tight_constraints(&self) -> BoxConstraints {
        let mut constraints = BoxConstraints::UNCONSTRAINED;
        if let Some(width) = self.width {
            constraints.min_width = px(width);
            constraints.max_width = px(width);
        }
        if let Some(height) = self.height {
            constraints.min_height = px(height);
            constraints.max_height = px(height);
        }
        constraints
    }
}

impl RenderView for SizedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderConstrainedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderConstrainedBox::new(self.tight_constraints())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_additional_constraints(self.tight_constraints());
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

impl_render_view!(SizedBox);
