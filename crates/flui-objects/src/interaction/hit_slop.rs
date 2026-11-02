//! `RenderHitSlop` — a single-child proxy whose hit area extends past its
//! bounds, as React Native's `hitSlop` does for a view.
//!
//! The area is the box grown by `slop` on each side, then grown further,
//! centred on the box, to at least the minimum target on each axis. Layout,
//! paint and semantics geometry are the child's: only hit-testing reads the
//! area.
//!
//! The object does not take hits in its area itself. It reports the area
//! through `RenderBox::hit_slop_area`, and `PipelineOwner::hit_test` resolves
//! it after the visual walk (flui-rendering ARCHITECTURE.md, "Expanded hit
//! areas are resolved after the visual walk").

use flui_foundation::Single;
use flui_foundation::geometry::{EdgeInsets, Point, Rect, Size};
use flui_interaction::GestureSettingsProvider;
use flui_interaction::events::PointerKind;
use flui_rendering::{RenderUpdateImpact, parent_data::BoxParentData, traits::RenderBox};

/// Where a [`RenderHitSlop`] reads its minimum target side.
#[derive(Debug, Clone, PartialEq)]
pub enum MinTarget {
    /// An authored length in logical pixels.
    Fixed(f64),
    /// The provider's current `GestureSettings::min_target` for the pointer
    /// kind asking, read on every hit test, so a live host update applies
    /// without a rebuild and a mouse and a finger each get their own.
    Settings(GestureSettingsProvider),
}

impl MinTarget {
    /// The current length for a pointer of `kind`, with a negative or
    /// non-finite one counted as zero.
    #[must_use]
    pub fn resolve(&self, kind: PointerKind) -> f64 {
        admitted_length(match self {
            Self::Fixed(length) => *length,
            Self::Settings(provider) => provider.min_target_for(kind),
        })
    }
}

impl From<f64> for MinTarget {
    fn from(length: f64) -> Self {
        Self::Fixed(length)
    }
}

impl From<GestureSettingsProvider> for MinTarget {
    fn from(provider: GestureSettingsProvider) -> Self {
        Self::Settings(provider)
    }
}

/// A render object whose child is hittable within an expanded area.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderHitSlop {
    slop: EdgeInsets,
    min_target: MinTarget,
    has_child: bool,
}

impl RenderHitSlop {
    /// Creates a proxy whose hit area is its box grown by `slop`, and at
    /// least `min_target` logical pixels on each axis.
    ///
    /// A negative, NaN or infinite inset or minimum counts as zero: the area
    /// never shrinks below the box.
    #[must_use]
    pub fn new(slop: EdgeInsets, min_target: impl Into<MinTarget>) -> Self {
        Self {
            slop: admitted_insets(slop),
            min_target: min_target.into(),
            has_child: false,
        }
    }

    /// The per-side growth of the hit area.
    #[must_use]
    pub const fn slop(&self) -> EdgeInsets {
        self.slop
    }

    /// Where the smallest side of the hit area comes from.
    #[must_use]
    pub const fn min_target(&self) -> &MinTarget {
        &self.min_target
    }

    /// Replaces the per-side growth. Hit-testing reads it on every walk, so
    /// no frame work follows.
    pub fn set_slop(&mut self, slop: EdgeInsets) -> RenderUpdateImpact {
        self.slop = admitted_insets(slop);
        RenderUpdateImpact::NONE
    }

    /// Replaces the minimum target source. Hit-testing reads it on every
    /// walk, so no frame work follows.
    pub fn set_min_target(&mut self, min_target: impl Into<MinTarget>) -> RenderUpdateImpact {
        self.min_target = min_target.into();
        RenderUpdateImpact::NONE
    }

    /// The hit area for a box of `size`, in local coordinates, as a pointer of
    /// `kind` sees it.
    #[must_use]
    pub fn area(&self, size: Size, kind: PointerKind) -> Rect {
        let min_target = self.min_target.resolve(kind);
        let grow = |extent: f64, before: f64, after: f64| {
            let shortfall = (min_target - (extent + before + after)).max(0.0) * 0.5;
            (before + shortfall, after + shortfall)
        };
        let (left, right) = grow(size.width, self.slop.left, self.slop.right);
        let (top, bottom) = grow(size.height, self.slop.top, self.slop.bottom);
        Rect::from_ltrb(-left, -top, size.width + right, size.height + bottom)
    }
}

fn admitted_length(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn admitted_insets(insets: EdgeInsets) -> EdgeInsets {
    EdgeInsets::new(
        admitted_length(insets.top),
        admitted_length(insets.right),
        admitted_length(insets.bottom),
        admitted_length(insets.left),
    )
}

impl flui_foundation::inspect::Inspect for RenderHitSlop {
    fn kind(&self) -> &'static str {
        "RenderHitSlop"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("slop", &self.slop)
            .unit(flui_foundation::inspect::Unit::LogicalPx);
        match &self.min_target {
            MinTarget::Fixed(length) => {
                out.field("min_target", &admitted_length(*length))
                    .unit(flui_foundation::inspect::Unit::LogicalPx);
            }
            MinTarget::Settings(_) => {
                for (name, kind) in [
                    ("min_target_touch", PointerKind::Touch),
                    ("min_target_mouse", PointerKind::Mouse),
                ] {
                    out.field(name, &self.min_target.resolve(kind))
                        .unit(flui_foundation::inspect::Unit::LogicalPx);
                }
            }
        }
    }
}

impl RenderBox for RenderHitSlop {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn hit_slop_area(&self, size: Size, kind: PointerKind) -> Option<Rect> {
        let area = self.area(size, kind);
        (area != Rect::from_origin_size(Point::ZERO, size)).then_some(area)
    }
}
