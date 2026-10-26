//! The system-bar overlay style a presentation's content asks for.
//!
//! After each painted frame the presentation probes the composited scene at
//! the top edge of its window for an annotated
//! [`SystemUiOverlayStyle`]. The style is staged until that frame is on
//! screen, so a refused, deferred or withheld frame announces nothing, and
//! delivered to the window's system chrome only when it differs from the last
//! delivery.
//! The native presentation detaching forgets the last delivery, so the next
//! frame after it attaches again resends.

use std::cell::Cell;

use flui_foundation::geometry::Point;
use flui_layer::LayerTree;
use flui_platform::{PlatformWindow, SystemUiOverlayStyle};

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
    /// delivery. Call once the probed frame is on screen.
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
            // Recorded before the call, so a reentrant delivery of the same
            // style is a no-op; a panic restores the previous delivery, and the
            // next accepted frame with this style tries again.
            let previous = self.delivered.replace(Some(style));
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                chrome.set_overlay_style(style);
            })) {
                if self.delivered.get() == Some(style) {
                    self.delivered.set(previous);
                }
                std::panic::resume_unwind(payload);
            }
        }
    }

    /// Forget the last delivery: the next probed style is sent again.
    pub(crate) fn forget_delivery(&self) {
        self.delivered.set(None);
    }
}
