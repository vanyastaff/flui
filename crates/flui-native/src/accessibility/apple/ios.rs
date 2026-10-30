//! UIAccessibility for iOS, via `accesskit_ios`'s low-level [`Adapter`]
//! (ADR-0255 decision 12).
//!
//! The window's `FluiView` is FLUI's own class, so it forwards UIKit's
//! container methods (`isAccessibilityElement`, `accessibilityElements`,
//! `accessibilityHitTest:`) to the adapter itself; nothing is subclassed at
//! run time. The view reaches the adapter through the accessibility slot
//! the window's reference carries, which this bridge fills as the window
//! opens.
//!
//! # Threads
//!
//! Every adapter method runs on the main thread, which is the owner thread
//! on iOS ("all adapter methods must be called on the main thread"), and
//! the adapter is held in a [`NativeOwner`], reachable there only. A
//! publish from any other thread keeps a self-contained update for the next
//! activation and drops the rest, as a publish with no assistive technology
//! attached does.
//!
//! # Two objc2 generations
//!
//! `accesskit_ios` is built on objc2 0.5 beside FLUI's 0.6. The two meet only
//! through raw pointers: the view goes in as a `*mut c_void`, the elements
//! and the hit element come back as autoreleased object pointers, and a
//! point crosses as its two coordinates.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;

use accesskit::TreeUpdate;
use accesskit_ios::Adapter;
use objc2::MainThreadMarker;

use super::adapter_cell::AdapterCell;
use crate::accessibility::bridge::{Action, Activation, BridgeShared, Deactivation};
use crate::owner::accessibility_slot::{ViewAccessibility, ViewPoint};
use crate::owner::apple::ios::{NativeOwner, ViewRef};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

/// The adapter as the window's view reaches it, through the slot its
/// [`ViewRef`] carries. Once the window's teardown took the adapter, the view
/// answers as a plain `UIView`.
struct ViewBridge(Rc<AdapterCell<Adapter>>);

impl ViewAccessibility for ViewBridge {
    fn is_accessibility_element(&self) -> Option<bool> {
        self.0.with(Adapter::is_accessibility_element)
    }

    fn accessibility_elements(&self) -> Option<*mut c_void> {
        self.0
            .with(|adapter| adapter.accessibility_elements().cast::<c_void>())
    }

    fn hit_test(&self, point: ViewPoint) -> Option<*mut c_void> {
        let point = accesskit_ios::CGPoint {
            x: point.x,
            y: point.y,
        };
        self.0
            .with(|adapter| adapter.hit_test(point).cast::<c_void>())
    }

    /// An assistive technology already running gets the tree now.
    fn did_move_to_window(&self) {
        if let Some(events) = self.0.with(Adapter::view_did_appear).flatten() {
            events.raise();
        }
    }
}

/// UIAccessibility for one window.
pub struct IosAccessibility {
    shared: Arc<BridgeShared>,
    /// The adapter, shared with the window's view, until `detach` takes it;
    /// after that a publish only retains.
    adapter: NativeOwner<Rc<AdapterCell<Adapter>>>,
}

/// Each UIKit window's view answers VoiceOver through its own adapter, made
/// as the window opens, on the main thread.
impl crate::host::Accessibility<ViewRef> for IosAccessibility {
    fn for_window(window: &ViewRef) -> Arc<dyn crate::host::WindowAccessibility> {
        let shared = Arc::new(BridgeShared::new());
        let view = objc2::rc::Retained::as_ptr(&window.view)
            .cast_mut()
            .cast::<c_void>();
        // SAFETY: `view` is the window's live `FluiView`, which the window
        // holds for its whole life; the adapter keeps only a weak reference
        // to it, so a view gone first leaves the adapter answering nothing.
        // The window makes its part on the main thread.
        let adapter = unsafe {
            Adapter::new(
                view,
                Activation(Arc::clone(&shared)),
                Action(Arc::clone(&shared)),
                Deactivation(Arc::clone(&shared)),
            )
        };
        let adapter = AdapterCell::new(adapter);
        window
            .accessibility
            .fill(Rc::new(ViewBridge(Rc::clone(&adapter))));
        Arc::new(Self {
            shared,
            adapter: NativeOwner::new(adapter, window.marker),
        })
    }
}

impl crate::host::WindowAccessibility for IosAccessibility {
    /// Drop the adapter on the main thread while the view is alive: now, or,
    /// when UIKit is inside it, as the outermost call returns
    /// (`AdapterCell::detach`). Idempotent.
    fn detach(&self) {
        if let Some(marker) = MainThreadMarker::new() {
            self.adapter.get(marker).detach();
        }
    }

    /// UIKit follows the key window itself.
    fn focus_changed(&self, _focused: bool) {}
}

impl PlatformAccessibility for IosAccessibility {
    fn publish(&self, update: TreeUpdate) {
        // Only self-contained updates may answer a late activation; see
        // `BridgeShared`.
        self.shared.retain_if_self_contained(&update);
        // The composition root publishes on the owner thread, which is the
        // main thread on iOS; anywhere else the update is only retained.
        let marker = MainThreadMarker::new();
        debug_assert!(
            marker.is_some(),
            "BUG: an iOS window's accessibility is published on its owner, the main thread"
        );
        let Some(marker) = marker else {
            return;
        };
        // Events are raised outside the adapter borrow: VoiceOver's
        // notification handlers can re-enter.
        let events = self
            .adapter
            .get(marker)
            .with(|adapter| adapter.update_if_active(move || update))
            .flatten();
        if let Some(events) = events {
            events.raise();
        }
    }

    fn is_active(&self) -> bool {
        self.shared.is_active()
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        self.shared.set_activation_listener(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        self.shared.set_action_listener(listener);
    }

    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        Self::ASSISTIVE_FOCUS
    }
}

impl IosAccessibility {
    /// `accesskit_ios` tracks VoiceOver's cursor itself and tells nobody
    /// ([gap 36](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#36-accessibility-bridge-fidelity-properties-and-announcements)).
    pub(crate) const ASSISTIVE_FOCUS: flui_semantics::AssistiveFocusSupport =
        flui_semantics::AssistiveFocusSupport::Unreported;
}

impl std::fmt::Debug for IosAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}
