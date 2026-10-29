//! A Win32 host of the owner's own for tests: the window hooks a test gives
//! it (none by default), sources that report nothing and a drag position that
//! takes no drags ([`QuietHost`]), and the windows of a loop that opens none
//! ([`NoWindows`]). The loop's tests run over it without the host table; two
//! of its instances are two hosts.

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, NullDataTransferSource,
    RepresentationIndex, TransferLimits, TransferRequest,
};
use flui_platform::drag::{DragError, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform::{DisplayId, DisplaySnapshot, SystemPreferences};

use super::broadcast::Subscriber;
use super::control::{
    OwnerLifecycle, OwnerMessages, OwnerObjects, OwnerPost, OwnerPreferences, OwnerSeeds,
    OwnerWindow,
};
use super::{
    ClassWindows, CreatedWindow, HostDisplays, HostDrag, WindowClass, WindowHooks, WindowInputs,
    WindowRef, WindowTexts, WindowsHost,
};
use crate::error::PlatformError;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::handlers::OpenWindows;
use crate::owner::notices::testing::NoNotices;
use crate::owner::notices::{NoticeChannel, StaleOnWake};
use crate::owner::sources::SystemPreferenceSource;
use flui_platform::text_store::TextStoreHost;

/// The hooks of a test host's window class, in the order the class's
/// procedure offers them a message.
pub(crate) trait ClassHooks: 'static {
    /// The hooks, made once per owner thread as the class is.
    fn hooks() -> WindowHooks;
}

/// A window class with no hooks: every message goes to `DefWindowProcW`.
pub(crate) struct NoHooks;

impl ClassHooks for NoHooks {
    fn hooks() -> WindowHooks {
        Rc::new([])
    }
}

/// A Win32 host whose window class has the hooks of `C` (none by default),
/// whose sources report nothing and whose drag position takes no drags.
pub(crate) struct QuietHost<C = NoHooks>(PhantomData<fn() -> C>);

impl<C: ClassHooks> WindowsHost for QuietHost<C> {
    fn window_class() -> WindowClass {
        WindowClass {
            hooks: C::hooks(),
            input: Rc::new(NoInputs),
            text: Rc::new(NoTexts),
        }
    }

    fn preferences(_: &OwnerSeeds<'_>) -> Result<Rc<dyn OwnerPreferences>, PlatformError> {
        Ok(Rc::new(NoPreferences))
    }

    fn notices(_: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
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
        Err(flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotRegistered).into())
    }
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
        Err(
            flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform)
                .into(),
        )
    }
}

impl HostDrag for NoDrag {
    fn drag_source(self: Arc<Self>, _window: WindowRef) -> Arc<dyn PlatformDragSource> {
        self
    }

    fn register_target(
        self: Arc<Self>,
        _window: &CreatedWindow,
    ) -> Option<Box<dyn super::DropTarget>> {
        None
    }
}

/// A text input that holds no window.
pub(crate) struct NoTexts;

impl WindowTexts for NoTexts {
    fn store_host(&self, _window: WindowRef) -> Option<Rc<dyn TextStoreHost>> {
        None
    }
}
