//! The headless host's text input: a recorder of the IME requests a window's
//! presentation sends.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_platform::PlatformTextInput;
use flui_platform::text_store::TextStoreHost;
use parking_lot::Mutex;

/// Recording fake for [`PlatformTextInput`], backing the headless backend's
/// [`PlatformWindow::text_input`](flui_platform::PlatformWindow::text_input).
///
/// Every `set_ime_allowed`/`set_ime_cursor_area` call is appended to an
/// in-memory history so a test can assert exactly what a presentation-owned
/// text-input session told the platform to do, rather than only that the call
/// didn't panic.
#[derive(Default)]
pub struct FakeTextInput {
    state: Mutex<FakeTextInputState>,
}

#[derive(Default, Clone)]
struct FakeTextInputState {
    ime_allowed_calls: Vec<bool>,
    cursor_area_calls: Vec<Bounds<f64>>,
}

impl FakeTextInput {
    /// Create a fresh recorder with no recorded calls.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `set_ime_allowed` call, in delivery order.
    #[must_use]
    pub fn ime_allowed_calls(&self) -> Vec<bool> {
        self.state.lock().ime_allowed_calls.clone()
    }

    /// The most recent `set_ime_allowed` call, if any.
    #[must_use]
    pub fn last_ime_allowed(&self) -> Option<bool> {
        self.state.lock().ime_allowed_calls.last().copied()
    }

    /// Every `set_ime_cursor_area` call, in delivery order.
    #[must_use]
    pub fn cursor_area_calls(&self) -> Vec<Bounds<f64>> {
        self.state.lock().cursor_area_calls.clone()
    }
}

impl std::fmt::Debug for FakeTextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("FakeTextInput")
            .field("ime_allowed_calls", &state.ime_allowed_calls)
            .field("cursor_area_calls", &state.cursor_area_calls)
            .finish()
    }
}

impl PlatformTextInput for FakeTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.state.lock().ime_allowed_calls.push(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        self.state.lock().cursor_area_calls.push(area);
    }
}

impl super::TextInput for FakeTextInput {
    fn ime(self: Arc<Self>) -> Option<Arc<dyn PlatformTextInput>> {
        Some(self)
    }

    fn store_host(self: Rc<Self>) -> Option<Rc<dyn TextStoreHost>> {
        None
    }
}
