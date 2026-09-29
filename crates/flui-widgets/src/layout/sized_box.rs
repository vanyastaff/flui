//! [`SizedBox`] — forces a specific size on its child (or itself).

use flui_objects::RenderConstrainedBox;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// A box with a specific size that forces its child to that size.
///
/// Flutter parity: `widgets/basic.dart` `SizedBox`. Like Flutter, this is a
/// `RenderConstrainedBox` whose additional constraints are tight for the given
/// width/height; an unset dimension passes the parent's constraint through on
/// that axis (`BoxConstraints.tightFor`).
#[derive(Clone, Debug, Default)]
pub struct SizedBox {
    width: Option<f64>,
    height: Option<f64>,
    child: Child,
}

impl SizedBox {
    /// A box that forces both `width` and `height` on its child.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
            child: Child::empty(),
        }
    }

    /// A square box of the given side length.
    pub fn square(dimension: f64) -> Self {
        Self::new(dimension, dimension)
    }

    /// A box that forces only its width; height passes through.
    pub fn width(width: f64) -> Self {
        Self {
            width: Some(width),
            height: None,
            child: Child::empty(),
        }
    }

    /// A box that forces only its height; width passes through.
    pub fn height(height: f64) -> Self {
        Self {
            width: None,
            height: Some(height),
            child: Child::empty(),
        }
    }

    /// A box that becomes as large as its parent allows (infinite tight on
    /// both axes — Flutter's `SizedBox.expand`).
    pub fn expand() -> Self {
        Self::new(f64::INFINITY, f64::INFINITY)
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
            constraints.min_width = width;
            constraints.max_width = width;
        }
        if let Some(height) = self.height {
            constraints.min_height = height;
            constraints.max_height = height;
        }
        constraints
    }
}

impl RenderView for SizedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderConstrainedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderConstrainedBox::new(self.tight_constraints())
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_additional_constraints(self.tight_constraints());
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SizedBox);
