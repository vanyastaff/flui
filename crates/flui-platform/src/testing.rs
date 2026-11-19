//! What test doubles of the platform contracts share (ADR-0243), compiled
//! only with the `testing` feature: production code has no use for any of it.
//!
//! [`window_takes_no_frame_verbs!`](crate::window_takes_no_frame_verbs) writes
//! the per-frame verbs of a [`PlatformWindow`](crate::PlatformWindow) double
//! whose window system takes neither a refresh-rate request nor input
//! regions, so each double does not repeat them; [`NoNativeSurface`] is the
//! surface target of a double with no native surface.
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

/// The per-frame verbs of a window double whose window system takes neither
/// a refresh-rate request nor input regions: both answer
/// [`Unsupported`](crate::Unsupported) with
/// [`NotOnThisPlatform`](crate::UnsupportedReason::NotOnThisPlatform), which
/// agrees with capabilities that leave
/// [`frame_rate_hint`](crate::WindowCapabilities::frame_rate_hint) and
/// every input region kind ([`input_region`](crate::input_capabilities::InputCapabilities::input_region))
/// `Unavailable`. Written inside an `impl PlatformWindow` block.
///
/// ```
/// # use std::sync::Arc;
/// # use flui_platform::*;
/// // Its id, and what it honours, published once when it was made.
/// struct Double(WindowId, WindowCapabilities);
///
/// impl PlatformWindow for Double {
///     flui_platform::window_takes_no_frame_verbs!();
/// #   fn id(&self) -> WindowId { self.0 }
/// #   fn request_redraw(&self) {}
/// #   fn surface_target(&self) -> Arc<dyn SurfaceTarget> { Arc::new(testing::NoNativeSurface) }
/// #   fn redraw_requester(&self) -> Arc<dyn RedrawRequester> { Arc::new(|| {}) }
/// #   fn state(&self) -> WindowState {
/// #       WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO).with_capabilities(self.1)
/// #   }
/// #   fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
/// #       command.against(&self.1)?;
/// #       Ok(())
/// #   }
/// #   fn set_cursor(&self, _: &Cursor) -> Result<(), CursorError> { Ok(()) }
/// #   fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> { None }
/// #   fn haptics(&self) -> Option<Arc<dyn haptics::PlatformHaptics>> { None }
/// #   fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> { None }
/// #   fn drag_source(&self) -> Option<Arc<dyn drag::PlatformDragSource>> { None }
/// #   fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
/// #       Ok(None)
/// #   }
/// #   fn window_at(&self, _: flui_foundation::geometry::Point<f64>) -> WindowAt {
/// #       WindowAt::Unknown
/// #   }
/// #   fn reveal_after_first_frame(&self) {}
/// }
///
/// # let double = Double(
/// #     WindowIdMint::new().next().expect("a fresh mint issues an id"),
/// #     WindowCapabilities::NONE.publish().expect("the process has publications left"),
/// # );
/// let refused = double.set_frame_rate_hint(FrameRateHint::High).unwrap_err();
/// assert_eq!(refused.reason, UnsupportedReason::NotOnThisPlatform);
/// assert!(double.set_input_regions(&[]).is_err());
/// ```
#[macro_export]
macro_rules! window_takes_no_frame_verbs {
    () => {
        fn set_frame_rate_hint(
            &self,
            _: $crate::FrameRateHint,
        ) -> ::core::result::Result<(), $crate::Unsupported> {
            ::core::result::Result::Err($crate::Unsupported::new(
                $crate::UnsupportedReason::NotOnThisPlatform,
            ))
        }

        fn set_input_regions(
            &self,
            _: &[$crate::input_regions::InputRegion],
        ) -> ::core::result::Result<(), $crate::Unsupported> {
            ::core::result::Result::Err($crate::Unsupported::new(
                $crate::UnsupportedReason::NotOnThisPlatform,
            ))
        }
    };
}
