//! A Win32 host of the owner's own for tests: no window hooks, sources that
//! report nothing and a drag position that takes no drags ([`QuietHost`]),
//! and the windows of a loop that opens none ([`NoWindows`]). The loop's tests
//! run over it without the host table, and a test that needs a second host
//! beside the Windows one names it.

use std::rc::Rc;
use std::sync::{Arc, Weak};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, NullDataTransferSource,
    RepresentationIndex, TransferLimits, TransferRequest,
};
use flui_platform::drag::{DragError, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform::{DisplayId, DisplaySnapshot, SystemNotice, SystemPreferences};

use super::broadcast::Subscriber;
use super::control::{
    OwnerLifecycle, OwnerMessages, OwnerObjects, OwnerPost, OwnerPreferences, OwnerSeeds,
    OwnerWindow,
};
use super::{
    ClassWindows, HostDisplays, HostDrag, WindowClass, WindowInputs, WindowMessages, WindowRef,
    WindowsHost,
};
use crate::error::PlatformError;
use crate::owner::handlers::OpenWindows;
use crate::owner::notices::{NoticeChannel, StaleOnWake, SystemNoticeHook};
use crate::owner::sources::SystemPreferenceSource;

/// A Win32 host whose window class has no hooks, whose sources report
/// nothing and whose drag position takes no drags.
pub(crate) struct QuietHost;

impl WindowsHost for QuietHost {
    fn window_class() -> WindowClass {
        WindowClass {
            hooks: Rc::new([]) as Rc<[Rc<dyn WindowMessages>]>,
            input: Rc::new(NoInputs),
        }
    }

    fn preferences(_: &OwnerSeeds<'_>) -> Result<Rc<dyn OwnerPreferences>, PlatformError> {
        Ok(Rc::new(NoPreferences))
    }

    fn notices() -> Arc<dyn NoticeChannel> {
        Arc::new(NoNotices)
    }

    fn lifecycle(_: &OwnerSeeds<'_>) -> Rc<dyn OwnerLifecycle> {
        Rc::new(NoLifecycle)
    }

    fn broadcast_subscribers(_: &OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>> {
        Vec::new()
    }

    fn displays() -> Arc<dyn HostDisplays> {
        Arc::new(NoDisplays)
    }

    fn drag() -> Arc<dyn HostDrag> {
        Arc::new(NoDrag)
    }
}

/// No window, for a loop a test builds without opening one.
#[derive(Debug)]
pub(crate) struct NoWindows;

impl OpenWindows for NoWindows {
    fn is_empty(&self) -> bool {
        true
    }
}

impl ClassWindows for NoWindows {
    fn redraw_for_deadline(&self) {}

    fn paint_invalid(&self) {}
}

/// An input that holds no window.
pub(crate) struct NoInputs;

impl WindowInputs for NoInputs {
    fn hovered(&self, _window: WindowRef) -> Option<bool> {
        None
    }

    fn modifiers(&self, _window: WindowRef) -> Option<flui_platform::Modifiers> {
        None
    }

    fn press_kind(
        &self,
        _window: WindowRef,
        _pointer: flui_platform::PointerId,
    ) -> Option<flui_platform::pointer::PointerKind> {
        None
    }

    fn pen_in_range(
        &self,
        _window: WindowRef,
        _device: Option<flui_platform::pointer::DeviceId>,
    ) -> Option<u64> {
        None
    }
}

/// System preferences that are never read and never change.
pub(crate) struct NoPreferences;

impl SystemPreferenceSource for NoPreferences {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        Ok(SystemPreferences::default())
    }
}

impl Subscriber for NoPreferences {
    fn message(&self, _: HWND, _: u32, _: WPARAM, _: LPARAM) -> Option<LRESULT> {
        None
    }
}

impl OwnerPreferences for NoPreferences {
    fn pending(&self) -> bool {
        false
    }

    fn retry_deadline(&self) -> Option<web_time::Instant> {
        None
    }

    fn retry_if_due(&self) {}

    fn broadcasts(&self) -> Rc<dyn Subscriber> {
        Rc::new(Self)
    }

    fn mouse_gestures(
        &self,
        _: u32,
    ) -> Result<flui_platform::GesturePreferences, flui_platform::PreferenceQueryError> {
        Err(flui_platform::PreferenceQueryError::Unavailable)
    }
}

/// A notice channel that keeps no hook and marks nothing stale.
pub(crate) struct NoNotices;

impl NoticeChannel for NoNotices {
    fn set_hook(&self, _: SystemNoticeHook) {}

    fn mark_held_stale_on_wake(&self, _: Weak<dyn StaleOnWake>) {}

    fn dispatch(&self, _: SystemNotice) {}
}

/// A lifecycle that hears no broadcast and holds no session end.
pub(crate) struct NoLifecycle;

impl Subscriber for NoLifecycle {
    fn message(&self, _: HWND, _: u32, _: WPARAM, _: LPARAM) -> Option<LRESULT> {
        None
    }
}

impl OwnerLifecycle for NoLifecycle {
    fn release_session_end(&self) {}
}

/// Displays the source holds none of.
pub(crate) struct NoDisplays;

impl StaleOnWake for NoDisplays {
    fn mark_stale(&self) {}
}

impl HostDisplays for NoDisplays {
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        Ok(DisplaySnapshot::empty())
    }

    fn invalidate(&self) {}

    fn display_of_window(&self, _: HWND) -> Option<DisplayId> {
        None
    }

    fn nearest_monitor(&self, _: HWND) -> Option<std::num::NonZeroIsize> {
        None
    }

    fn sample_count(&self) -> usize {
        0
    }
}

/// A transport that takes and starts no drags, and leaves the owner window's
/// messages to the rest.
pub(crate) struct NoDrag;

impl OwnerMessages for NoDrag {
    fn attach(&self, _owner: OwnerPost) {}

    fn message(&self, _owner: &OwnerWindow, _message: u32) -> Option<LRESULT> {
        None
    }

    fn destroyed(&self) {}
}

impl DataTransferSource for NoDrag {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        NullDataTransferSource.request(id, representation, limits)
    }

    fn update_drop_feedback(&self, _id: DataTransferId, _feedback: DropFeedback) {}

    fn conclude_drop(&self, _id: DataTransferId) {}
}

impl PlatformDragSource for NoDrag {
    fn start_timing(&self) -> OsDragStart {
        OsDragStart::OnLeavingWindow
    }

    fn start(&self, _request: OsDragRequest) -> Result<(), DragError> {
        Err(DragError::Unavailable)
    }
}

impl HostDrag for NoDrag {
    fn drag_source(self: Arc<Self>, _window: WindowRef) -> Arc<dyn PlatformDragSource> {
        self
    }

    fn register_target(self: Arc<Self>, _window: WindowRef) {}
}
