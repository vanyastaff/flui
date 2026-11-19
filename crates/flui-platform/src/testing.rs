//! What test doubles of the platform contracts share (ADR-0243), compiled
//! only with the `testing` feature: production code has no use for any of it.
//!
//! [`MockWindow`] is the one double of [`PlatformWindow`](crate::PlatformWindow):
//! a test that needs a window and not a host builds it, standalone or over a
//! host's window, instead of implementing the trait; [`NoNativeSurface`] is
//! the surface target of a window with no native surface.
//!
//! The doubles of the contracts this crate owns (ADR-0243 D4): `FakeHaptics`, the recording
//! [`PlatformHaptics`](crate::haptics::PlatformHaptics) a scenario asserts against,
//! `ScriptedHttp`, the [`HttpClient`](crate::http::HttpClient) a headless host
//! registers so no test reaches the network, and
//! `ScriptedLauncher`, the scripted [`LauncherBackend`](crate::launch::LauncherBackend), and
//! `MemoryAssets`, the [`AssetSource`](crate::assets::AssetSource) a test hands its host
//! its assets through.
//!
//! A double whose service answers later is built on the scenario primitive,
//! [`capability::scripted`](crate::capability::scripted), so its answers are
//! matched by request, missing ones are typed failures and time is the
//! scenario's clock. Three doubles are not, each for its contract's sake:
//! `FakeHaptics` answers synchronously through a window's trait object, with
//! no completion to settle; `ScriptedHttp` serves a `Send + Sync` client
//! whose futures run off the owner thread (ADR-0280), where an owner-local
//! scenario cannot reach, so it fails a URL nobody scripted with
//! [`HttpError::Connect`](crate::http::HttpError::Connect) naming it and,
//! attached to a scenario (`ScriptedHttp::report_to`), reports it into the
//! scenario's ledger; and `flui_testing::storage::MemoryStorage` is a store,
//! not a script, behind the `Send + Sync` storage contract, with nothing a
//! request could be unexpected against.

mod assets;

pub use assets::MemoryAssets;

mod haptics;

pub use haptics::{FakeHaptics, HapticCall, HapticPlay};

mod http;

pub use http::{ScriptedHttp, ScriptedResponse};

mod launcher;

pub use launcher::{LaunchRequest, ScriptedLauncher};

pub mod mock_window;

pub use mock_window::{MockWindow, WindowCall};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

/// The surface target of a window double with no native surface: both handle
/// queries answer [`HandleError::Unavailable`], and
/// [`pre_present_notify`](crate::SurfaceTarget::pre_present_notify) returns at
/// once, since nothing presents into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoNativeSurface;

impl HasWindowHandle for NoNativeSurface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for NoNativeSurface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl crate::SurfaceTarget for NoNativeSurface {
    /// Nothing presents into a surface that does not exist.
    fn pre_present_notify(
        &self,
        _: Option<flui_foundation::FrameStamp>,
        _: crate::SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}
