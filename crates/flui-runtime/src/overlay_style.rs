//! The system-bar overlay style a presentation's content asks for.
//!
//! After each painted frame the presentation probes the composited scene at
//! the top edge of its window for an annotated
//! [`SystemUiOverlayStyle`]. The style is staged until the sink accepts that
//! frame, so a refused or deferred frame announces nothing, and delivered to
//! the window's system chrome only when it differs from the last delivery.
//! The native presentation detaching forgets the last delivery, so the next
//! frame after it attaches again resends.

use std::cell::Cell;

use flui_foundation::geometry::Point;
use flui_layer::LayerTree;
use flui_platform_api::{PlatformWindow, SystemUiOverlayStyle};

/// Coalesces per-frame overlay-style probes into changes for the platform.
#[derive(Debug, Default)]
pub(crate) struct OverlayStyleCoalescer {
    staged: Cell<Option<SystemUiOverlayStyle>>,
    delivered: Cell<Option<SystemUiOverlayStyle>>,
}

impl OverlayStyleCoalescer {
    /// Probe `tree` at the middle of the top edge of a `width`-wide window.
    /// A frame without an overlay annotation stages nothing and leaves the
    /// platform's current style alone.
    pub(crate) fn stage(&self, tree: &LayerTree, width: f64) {
        let probe = Point::new(width * 0.5, 0.0);
        self.staged.set(
            tree.find_annotation::<SystemUiOverlayStyle>(probe)
                .map(|style| *style),
        );
    }

    /// Deliver the staged style to `window` if it changed since the last
    /// delivery. Call once the sink has accepted the probed frame.
    pub(crate) fn deliver(&self, window: &dyn PlatformWindow) {
        let Some(style) = self.staged.take() else {
            return;
        };
        if self.delivered.get() == Some(style) {
            return;
        }
        // Without the capability nothing was delivered, so a backend that
        // gains it later still receives the current style.
        if let Some(chrome) = window.system_chrome() {
            self.delivered.set(Some(style));
            chrome.set_overlay_style(style);
        }
    }

    /// Forget the last delivery: the next probed style is sent again.
    pub(crate) fn forget_delivery(&self) {
        self.delivered.set(None);
    }
}
