//! The display a presentation's window is on (ADR-0236).
//!
//! The host derives a window's [`WindowDisplay`] from the platform's display
//! snapshot and delivers it as a window observation. The presentation stores
//! the last one it applied; readers (the pacing fallback, the frame budget, the
//! pointer resampler, the refresh-interval metric) take the value from here
//! rather than asking the platform.

use flui_foundation::PresentationId;
use flui_platform_api::WindowDisplay;

use super::UiRuntime;

impl UiRuntime {
    /// The display presentation `id`'s window is on, as the host last delivered
    /// it: `None` while the window names no display, one the platform's snapshot
    /// does not hold, or the presentation is not hosted here.
    #[must_use]
    pub fn window_display(&self, id: PresentationId) -> Option<WindowDisplay> {
        self.presentations.get(id)?.window_display.get()
    }

    /// Apply the display the host derived for presentation `id`'s window.
    /// Returns whether it differs from the one already applied; an unchanged
    /// value does nothing, so a host may deliver its derivation as often as it
    /// likes.
    ///
    /// A change asks for a redraw first and is stored last: if the redraw
    /// request fails part-way, the presentation still holds the previous value,
    /// and the host's next delivery, even of an equal value, applies it again.
    pub(crate) fn apply_window_display(
        &self,
        id: PresentationId,
        display: Option<WindowDisplay>,
    ) -> bool {
        let Some(presentation) = self.presentations.get(id) else {
            return false;
        };
        if presentation.window_display.get() == display {
            return false;
        }
        self.request_redraw_for(presentation);
        presentation.window_display.set(display);
        true
    }
}
