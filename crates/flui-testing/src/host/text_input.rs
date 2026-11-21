//! The recording IME capability of a headless window.

#[cfg(doc)]
use super::HeadlessWindow;

use flui_foundation::geometry::Bounds;
use flui_platform::PlatformTextInput;
use flui_platform::text::store::TextStoreStatus;

use parking_lot::Mutex;

/// The IME capability of a [`HeadlessWindow`] built with a text input.
#[derive(Debug, Default)]
// `Mutex` fields: `PlatformTextInput` is `Send + Sync`.
pub(super) struct RecordingTextInput {
    pub(super) cursor_areas: Mutex<Vec<Bounds<f64>>>,
    pub(super) ime_allowed: Mutex<Vec<bool>>,
    pub(super) field_statuses: Mutex<Vec<TextStoreStatus>>,
}

impl PlatformTextInput for RecordingTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.ime_allowed.lock().push(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        self.cursor_areas.lock().push(area);
    }

    fn set_field_status(&self, status: TextStoreStatus) {
        self.field_statuses.lock().push(status);
    }

    /// No input method composes in a headless window.
    fn discard_composition(&self) {}
}
