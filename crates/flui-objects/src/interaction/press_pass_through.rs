//! `RenderPressPassThrough` — a single-child proxy whose subtree's presses move the window
//! as the application's title bar around it does (ADR-0286).
//!
//! Under a move area the input-region walk cuts every press-claiming node out as window
//! content. A subtree that listens to presses but should still drag the window, a
//! decoration that only animates on a press for instance, is wrapped in
//! this object. Layout, paint and hit-testing are the child's.

use flui_foundation::Single;
use flui_foundation::geometry::Size;
use flui_rendering::input_regions::InputRegionSink;
use flui_rendering::{RenderUpdateImpact, parent_data::BoxParentData, traits::RenderBox};

/// A render object whose subtree's presses move the window while it is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPressPassThrough {
    enabled: bool,
    has_child: bool,
}

impl RenderPressPassThrough {
    /// Passes the subtree's presses through to the move area around it.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            enabled: true,
            has_child: false,
        }
    }

    /// Whether presses pass through now.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Passes presses through, or stops. The input regions are collected on composited
    /// frames, so a change marks paint.
    pub fn set_enabled(&mut self, enabled: bool) -> RenderUpdateImpact {
        if self.enabled == enabled {
            return RenderUpdateImpact::NONE;
        }
        self.enabled = enabled;
        RenderUpdateImpact::PAINT
    }
}

impl Default for RenderPressPassThrough {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::inspect::Inspect for RenderPressPassThrough {
    fn kind(&self) -> &'static str {
        "RenderPressPassThrough"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("disabled", &!self.enabled).default(&false);
    }
}

impl RenderBox for RenderPressPassThrough {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn describe_input_regions(&self, _size: Size, regions: &mut InputRegionSink) {
        if self.enabled {
            regions.pass_presses_through();
        }
    }
}
