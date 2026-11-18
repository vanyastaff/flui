//! `RenderInputRegion` — a single-child proxy that claims its box as an input region of
//! one kind, such as system-gesture exclusion (ADR-0217 §5).
//!
//! Layout, paint and hit-testing are the child's. The object reports its own box through
//! `RenderBox::describe_input_regions`, and `PipelineOwner::input_regions` maps it into the
//! window on every composited frame.

use flui_foundation::Single;
use flui_foundation::geometry::{Point, Rect, Size};
use flui_interaction::pointer::FirstClickTarget;
use flui_rendering::hit_testing::HitMetadata;
use flui_rendering::input_regions::{InputRegionKind, InputRegionSink};
use flui_rendering::{RenderUpdateImpact, parent_data::BoxParentData, traits::RenderBox};

/// A render object whose box is a `kind` input region while it is enabled.
///
/// An enabled [`InputRegionKind::FirstClick`] region also marks every hit that lands in its
/// child with a [`FirstClickTarget`], which is how the gesture binding knows a press marked
/// first click reached this box rather than a widget painted over it.
#[derive(Debug, Clone)]
pub struct RenderInputRegion {
    kind: InputRegionKind,
    enabled: bool,
    has_child: bool,
    /// The [`FirstClickTarget`] payload while the box accepts the first click, made once so
    /// that a hit test allocates nothing.
    first_click: Option<HitMetadata>,
}

impl PartialEq for RenderInputRegion {
    fn eq(&self, other: &Self) -> bool {
        // `first_click` follows from the other two fields.
        self.kind == other.kind
            && self.enabled == other.enabled
            && self.has_child == other.has_child
    }
}

impl Eq for RenderInputRegion {}

impl RenderInputRegion {
    /// Claims this object's box as a `kind` region.
    #[must_use]
    pub fn new(kind: InputRegionKind) -> Self {
        let mut region = Self {
            kind,
            enabled: true,
            has_child: false,
            first_click: None,
        };
        region.refresh_first_click();
        region
    }

    /// Makes the hit payload agree with the kind and the enabled flag.
    fn refresh_first_click(&mut self) {
        let accepts = self.enabled && self.kind == InputRegionKind::FirstClick;
        if accepts != self.first_click.is_some() {
            self.first_click = accepts.then(|| HitMetadata::new(FirstClickTarget));
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
        self.refresh_first_click();
        RenderUpdateImpact::PAINT
    }

    /// Claims the box, or stops claiming it. A change marks paint so that a frame is
    /// composited and the window receives the new set.
    pub fn set_enabled(&mut self, enabled: bool) -> RenderUpdateImpact {
        if self.enabled == enabled {
            return RenderUpdateImpact::NONE;
        }
        self.enabled = enabled;
        self.refresh_first_click();
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
    const REPORTS_INPUT_REGIONS: bool = true;

    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    /// A hit in the child of an enabled first-click region carries the
    /// [`FirstClickTarget`].
    fn metadata(&self) -> Option<HitMetadata> {
        self.first_click.clone()
    }

    fn describe_input_regions(&self, size: Size, regions: &mut InputRegionSink) {
        if self.enabled {
            regions.add(self.kind, Rect::from_origin_size(Point::ZERO, size));
        }
    }
}
