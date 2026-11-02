//! `RenderInputRegion` — a single-child proxy that claims its box as an input region of
//! one kind, such as system-gesture exclusion (ADR-0217 §5).
//!
//! Layout, paint and hit-testing are the child's. The object reports its own box through
//! `RenderBox::describe_input_regions`, and `PipelineOwner::input_regions` maps it into the
//! window on every composited frame.

use flui_foundation::Single;
use flui_foundation::geometry::{Point, Rect, Size};
use flui_rendering::input_regions::{InputRegionKind, InputRegionSink};
use flui_rendering::{RenderUpdateImpact, parent_data::BoxParentData, traits::RenderBox};

/// A render object whose box is a `kind` input region while it is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderInputRegion {
    kind: InputRegionKind,
    enabled: bool,
    has_child: bool,
}

impl RenderInputRegion {
    /// Claims this object's box as a `kind` region.
    #[must_use]
    pub const fn new(kind: InputRegionKind) -> Self {
        Self {
            kind,
            enabled: true,
            has_child: false,
        }
    }

    /// What the region is for.
    #[must_use]
    pub const fn kind(&self) -> InputRegionKind {
        self.kind
    }

    /// Whether the box is claimed now.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Replaces the region's kind. The set is collected on composited frames, so a
    /// change marks paint.
    pub fn set_kind(&mut self, kind: InputRegionKind) -> RenderUpdateImpact {
        if self.kind == kind {
            return RenderUpdateImpact::NONE;
        }
        self.kind = kind;
        RenderUpdateImpact::PAINT
    }

    /// Claims the box, or stops claiming it. A change marks paint so that a frame is
    /// composited and the window receives the new set.
    pub fn set_enabled(&mut self, enabled: bool) -> RenderUpdateImpact {
        if self.enabled == enabled {
            return RenderUpdateImpact::NONE;
        }
        self.enabled = enabled;
        RenderUpdateImpact::PAINT
    }
}

impl flui_foundation::inspect::Inspect for RenderInputRegion {
    fn kind(&self) -> &'static str {
        "RenderInputRegion"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("kind", &self.kind);
        out.field("disabled", &!self.enabled).default(&false);
    }
}

impl RenderBox for RenderInputRegion {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn describe_input_regions(&self, size: Size, regions: &mut InputRegionSink) {
        if self.enabled {
            regions.add(self.kind, Rect::from_origin_size(Point::ZERO, size));
        }
    }
}
