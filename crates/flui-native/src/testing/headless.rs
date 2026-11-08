//! Recording doubles for the headless host. They are installed only by
//! [`crate::HeadlessPlatform::with_test_doubles`].

use std::sync::Arc;

use flui_platform::{PlatformSystemChrome, PlatformTextInput};
use flui_semantics::platform::{
    AccessibilityAlreadyAttached, AccessibilityAttachment, AccessibilityHandler,
    PlatformAccessibility, RouteChange,
};
use parking_lot::Mutex;

use crate::accessibility::headless::HeadlessAccessibility;
use crate::text::{TextInput, WindowTextInput};

/// Counts trees published while active.
#[derive(Default)]
pub struct FakeAccessibility {
    bridge: HeadlessAccessibility,
    published: Mutex<usize>,
}

impl FakeAccessibility {
    /// Create a fresh recorder, inactive and with nothing published.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many trees were published while active.
    #[must_use]
    pub fn published_count(&self) -> usize {
        *self.published.lock()
    }

    /// Simulate assistive technology attaching or detaching. Its listener runs
    /// after the state lock is released, so it may safely re-enter this fake.
    pub fn set_active(&self, active: bool) {
        self.bridge.set_active(active);
    }
}

impl std::fmt::Debug for FakeAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeAccessibility")
            .field("bridge", &self.bridge)
            .field("published", &self.published)
            .finish()
    }
}

impl PlatformAccessibility for FakeAccessibility {
    fn publish(&self, update: accesskit::TreeUpdate) {
        if self.bridge.is_active() {
            let mut published = self.published.lock();
            *published = published.saturating_add(1);
        }
        self.bridge.publish(update);
    }

    fn attach(
        &self,
        handler: Arc<dyn AccessibilityHandler>,
    ) -> Result<AccessibilityAttachment, AccessibilityAlreadyAttached> {
        self.bridge.attach(handler)
    }

    fn detach_handler(&self) {
        self.bridge.detach_handler();
    }

    fn route_changed(&self, change: RouteChange) {
        self.bridge.route_changed(change);
    }
}

impl crate::host::WindowAccessibility for FakeAccessibility {
    fn detach(&self) {
        self.bridge.detach_handler();
    }

    fn focus_changed(&self, _: bool) {}
}

/// Records text-input calls in delivery order.
#[derive(Default)]
pub struct FakeTextInput {
    state: Mutex<FakeTextInputState>,
}

#[derive(Default)]
struct FakeTextInputState {
    ime_allowed_calls: Vec<bool>,
    cursor_area_calls: Vec<flui_foundation::geometry::Bounds<f64>>,
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
    pub fn cursor_area_calls(&self) -> Vec<flui_foundation::geometry::Bounds<f64>> {
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

    fn set_ime_cursor_area(&self, area: flui_foundation::geometry::Bounds<f64>) {
        self.state.lock().cursor_area_calls.push(area);
    }

    fn set_field_status(&self, _: flui_platform::text_store::TextStoreStatus) {}

    fn discard_composition(&self) {}
}

impl TextInput for Arc<FakeTextInput> {
    fn window_text_input(&self) -> Option<WindowTextInput> {
        Some(WindowTextInput::Ime(
            Arc::clone(self) as Arc<dyn PlatformTextInput>
        ))
    }

    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    fn invalidate_sizing(&self) {}

    fn fence_sizing(&self) {}

    fn detach(&self) {}
}

/// Records delivered system-overlay styles in delivery order.
#[derive(Default)]
pub struct FakeSystemChrome {
    styles: Mutex<Vec<flui_platform::SystemUiOverlayStyle>>,
}

impl FakeSystemChrome {
    /// Every delivered overlay style, in delivery order.
    #[must_use]
    pub fn styles(&self) -> Vec<flui_platform::SystemUiOverlayStyle> {
        self.styles.lock().clone()
    }
}

impl std::fmt::Debug for FakeSystemChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeSystemChrome")
            .field("styles", &*self.styles.lock())
            .finish()
    }
}

impl PlatformSystemChrome for FakeSystemChrome {
    fn set_overlay_style(&self, style: flui_platform::SystemUiOverlayStyle) {
        self.styles.lock().push(style);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
