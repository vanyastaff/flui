//! The host-side window: a [`PlatformWindow`] plus what only a composition
//! root may reach.
//!
//! [`PlatformWindow`] is the per-window contract the framework programs
//! against, and it names no AccessKit type (ADR-0082 §1). The accessibility
//! bridge speaks AccessKit, so it is reached through this subtrait instead:
//! [`Platform::open_window`](crate::traits::Platform::open_window),
//! [`WindowOpen::Ready`](crate::traits::WindowOpen::Ready) and
//! [`PendingWindow`](crate::traits::PendingWindow) hand the runner an
//! `Arc<dyn HostWindow>`, the runner reads [`HostWindow::accessibility`] once
//! and passes the window on as an `Arc<dyn PlatformWindow>` (an upcast).
//!
//! Every backend fixes its bridge when it builds the window, so reading it
//! once at open time sees the same bridge a later read would.
//!
//! The window's text-store host (ADR-0135) is owner-thread state, so it is
//! not read here directly: [`HostWindow::text_store_host`] takes an
//! `OwnerThreadToken` that only
//! [`OwnerPlatform::text_store_host`](crate::OwnerPlatform::text_store_host)
//! mints, on the thread that owns the window.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform_api::text_store::TextStoreHost;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use super::{PlatformAccessibility, PlatformWindow};

mod sealed {
    use std::marker::PhantomData;

    /// Proof that a call runs on the platform's owner thread: minted only by
    /// [`OwnerPlatform::text_store_host`](crate::OwnerPlatform::text_store_host),
    /// which only that thread can hold, and itself neither `Send` nor
    /// `Sync`. Public in a private module, so no other crate can name or
    /// build one, and the method that takes it cannot be called outside
    /// this crate.
    ///
    /// It proves the owner thread, not the window's thread: a backend
    /// whose windows belong to the thread that created them (Win32) checks
    /// that itself before it hands out a host.
    #[derive(Debug)]
    pub struct OwnerThreadToken(PhantomData<*const ()>);

    impl OwnerThreadToken {
        pub(crate) const fn new() -> Self {
            Self(PhantomData)
        }
    }
}

pub(crate) use sealed::OwnerThreadToken;

/// A window as a backend hands it to the composition root.
///
/// Implemented by every backend window next to its [`PlatformWindow`] impl.
/// A test double that is only ever handed to the framework needs just
/// [`PlatformWindow`]; one returned from `open_window` needs this too.
pub trait HostWindow: PlatformWindow {
    /// Acquire this presentation's native numeric converter on the owner lane.
    /// Pending acquisition owns a completion wake obligation; no frame path waits.
    /// The hidden token is minted by `OwnerPlatform::capture_text_sizing`.
    ///
    /// # Errors
    /// Refuses a closed presentation, wrong owner or failed native capture.
    fn capture_text_sizing(
        &self,
        _owner: OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    /// This window's accessibility bridge, if the backend exposes one.
    ///
    /// `None` for a backend with no accessibility integration — which is
    /// every backend until its per-OS adapter is wired, and permanently for
    /// one with no such platform API. A composition root that gets `None`
    /// never enables semantics assembly, so the cost is not paid either.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        None
    }

    /// This window's text-store host (ADR-0135), for a backend whose input
    /// methods pull from the focused field's store; `None` for a push-model
    /// backend (it offers [`PlatformWindow::text_input`]) or one with no
    /// input-method integration.
    ///
    /// The token proves the platform's owner thread; an implementation whose
    /// windows are bound to their creating thread checks that the caller is
    /// on it, and answers `None` otherwise.
    ///
    /// Callable only inside this crate, because only
    /// [`OwnerPlatform::text_store_host`](crate::OwnerPlatform::text_store_host)
    /// can build the token. Outside it, the token's type is not exported
    /// beside this trait. `trybuild_ui::ui_tests` pins both the private-import
    /// and missing-`Default` diagnostics and a valid owner-capability caller:
    ///
    /// ```compile_fail,E0603
    /// use flui_platform::traits::OwnerThreadToken;
    /// ```
    ///
    /// nor conjured:
    ///
    /// ```compile_fail,E0277
    /// fn read(window: &dyn flui_platform::traits::HostWindow) {
    ///     let _ = window.text_store_host(Default::default());
    /// }
    /// ```
    fn text_store_host(&self, _owner: OwnerThreadToken) -> Option<Rc<dyn TextStoreHost>> {
        None
    }
}

// Without these, `Arc<dyn HostWindow>` would not be a raw-handle target, and a
// renderer built straight from an `open_window` result (before the upcast)
// would not compile.
impl HasWindowHandle for dyn HostWindow + '_ {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        PlatformWindow::window_handle(self)
    }
}

impl HasDisplayHandle for dyn HostWindow + '_ {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        PlatformWindow::display_handle(self)
    }
}
