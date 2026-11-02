//! `RenderContentShape` — a single-child proxy whose hit area is a shape
//! instead of its bounding box, without clipping what it paints.
//!
//! SwiftUI's `contentShape` is the prior art: the shape, not the content, is
//! what pointers hit. Inside the shape the proxy takes the hit even where its
//! child paints nothing; outside it the position falls through to what is
//! below, and the child is not tested.
//!
//! A clip (`RenderClipOval` …) also hit-tests by its shape, but it clips paint
//! and opens a layer to do so. This object changes hit-testing only: layout,
//! paint and semantics geometry are the child's.

use std::sync::Arc;

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Point, RRect, Rect, Size};
use flui_painting::{paint::Path, styling::BorderRadius};
use flui_rendering::{
    RenderUpdateImpact, context::BoxHitTestContext, parent_data::BoxParentData, traits::RenderBox,
};

use crate::Oval;

/// The shape a [`RenderContentShape`] hit-tests by, in its local coordinates.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum HitShape {
    /// The whole box (a plain rectangle).
    Rect,
    /// The box with these corner radii.
    RoundedRect(BorderRadius),
    /// The ellipse inscribed in the box — a circle for a square box.
    Oval,
    /// An arbitrary path in local coordinates, filled by its fill rule. It is
    /// not scaled to the box; parts outside the box are not hittable.
    Path(Arc<Path>),
}

impl HitShape {
    /// Whether `point` lies in this shape laid over a box of `size` at the
    /// origin.
    #[must_use]
    pub fn contains(&self, size: Size, point: Point<f64>) -> bool {
        let bounds = Rect::from_origin_size(Point::ZERO, size);
        if !bounds.contains(point) {
            return false;
        }
        match self {
            Self::Rect => true,
            Self::RoundedRect(radius) => RRect::from_rect_and_corners(
                bounds,
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left,
            )
            .contains(point),
            Self::Oval => Oval::from_rect(bounds).contains(point),
            Self::Path(path) => path.contains(point),
        }
    }
}

/// A render object that hit-tests by a [`HitShape`] rather than its bounds.
#[derive(Debug, Clone)]
pub struct RenderContentShape {
    shape: HitShape,
    has_child: bool,
}

impl RenderContentShape {
    /// Creates a proxy that hit-tests by `shape`.
    #[must_use]
    pub const fn new(shape: HitShape) -> Self {
        Self {
            shape,
            has_child: false,
        }
    }

    /// The shape this object hit-tests by.
    #[must_use]
    pub const fn shape(&self) -> &HitShape {
        &self.shape
    }

    /// Replaces the shape. Hit-testing reads it on every walk, so no frame
    /// work follows.
    pub fn set_shape(&mut self, shape: HitShape) -> RenderUpdateImpact {
        self.shape = shape;
        RenderUpdateImpact::NONE
    }
}

impl flui_foundation::inspect::Inspect for RenderContentShape {
    fn kind(&self) -> &'static str {
        "RenderContentShape"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        use flui_foundation::inspect::{Unit, Value};
        // The variant is the shape's identity; a rounded rectangle adds its four corner radii.
        // An arbitrary path is only named: its outline has no typed value.
        out.value(
            "shape",
            Value::Variant {
                ty: "HitShape",
                name: match &self.shape {
                    HitShape::Rect => "Rect",
                    HitShape::RoundedRect(_) => "RoundedRect",
                    HitShape::Oval => "Oval",
                    HitShape::Path(_) => "Path",
                },
            },
        );
        if let HitShape::RoundedRect(radius) = &self.shape {
            for (name, corner) in [
                ("radius_top_left", radius.top_left),
                ("radius_top_right", radius.top_right),
                ("radius_bottom_right", radius.bottom_right),
                ("radius_bottom_left", radius.bottom_left),
            ] {
                out.field(name, &Size::new(corner.x, corner.y))
                    .unit(Unit::LogicalPx);
            }
        }
    }
}

impl RenderBox for RenderContentShape {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !self
            .shape
            .contains(ctx.own_size(), Point::new(ctx.x(), ctx.y()))
        {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO);
        }
        // The shape is the hit area: inside it this node takes the hit even
        // where the child paints nothing.
        true
    }
}
