//! The headless host's production accessibility sink.

use std::sync::Arc;

use flui_semantics::platform::{
    AccessibilityAlreadyAttached, AccessibilityAttachment, AccessibilityCapabilities,
    AccessibilityHandler, PlatformAccessibility, RouteChange,
};
use parking_lot::Mutex;

#[derive(Default)]
struct AccessibilityState {
    active: bool,
    handler: Option<Arc<dyn AccessibilityHandler>>,
}

/// The production headless accessibility bridge. Published trees are dropped;
/// no tree or publication history is retained.
#[derive(Default)]
pub struct HeadlessAccessibility {
    state: Mutex<AccessibilityState>,
}

impl HeadlessAccessibility {
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn set_active(&self, active: bool) {
        let handler = {
            let mut state = self.state.lock();
            if state.active == active {
                return;
            }
            state.active = active;
            state.handler.clone()
        };
        if let Some(handler) = handler {
            handler.activation_changed(active);
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn is_active(&self) -> bool {
        self.state.lock().active
    }
}

impl std::fmt::Debug for HeadlessAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessAccessibility")
            .field("active", &self.state.lock().active)
            .finish_non_exhaustive()
    }
}

impl PlatformAccessibility for HeadlessAccessibility {
    fn publish(&self, update: accesskit::TreeUpdate) {
        drop(update);
    }

    fn attach(
        &self,
        handler: Arc<dyn AccessibilityHandler>,
    ) -> Result<AccessibilityAttachment, AccessibilityAlreadyAttached> {
        let mut state = self.state.lock();
        if state.handler.is_some() {
            return Err(AccessibilityAlreadyAttached);
        }
        let attachment =
            AccessibilityAttachment::new(state.active, AccessibilityCapabilities::NONE);
        state.handler = Some(handler);
        Ok(attachment)
    }

    fn detach_handler(&self) {
        let handler = self.state.lock().handler.take();
        drop(handler);
    }

    fn route_changed(&self, change: RouteChange) {
        drop(change);
    }
}

impl crate::host::WindowAccessibility for HeadlessAccessibility {
    fn detach(&self) {
        self.detach_handler();
    }

    fn focus_changed(&self, _: bool) {}
}

impl crate::host::Accessibility<()> for HeadlessAccessibility {
    fn for_window((): &()) -> std::sync::Arc<dyn crate::host::WindowAccessibility> {
        std::sync::Arc::new(Self::default())
    }
}
