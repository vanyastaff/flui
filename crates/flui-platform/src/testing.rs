//! What test doubles of the window contract share (ADR-0243).
//!
//! [`window_takes_no_frame_verbs!`](crate::window_takes_no_frame_verbs) writes
//! the per-frame verbs of a [`PlatformWindow`](crate::PlatformWindow) double
//! whose window system takes neither a refresh-rate request nor input
//! regions, so each double does not repeat them.

/// The per-frame verbs of a window double whose window system takes neither
/// a refresh-rate request nor input regions: both answer
/// [`Unsupported`](crate::Unsupported) with
/// [`NotOnThisPlatform`](crate::UnsupportedReason::NotOnThisPlatform), which
/// agrees with capabilities that leave
/// [`frame_rate_hint`](crate::WindowCapabilities::frame_rate_hint) and
/// [`input_regions`](crate::input_capabilities::InputCapabilities::input_regions)
/// `Unavailable`. Written inside an `impl PlatformWindow` block.
///
/// ```
/// # use std::sync::Arc;
/// # use flui_foundation::geometry::Size;
/// # use flui_platform::*;
/// struct Double;
///
/// impl PlatformWindow for Double {
///     flui_platform::window_takes_no_frame_verbs!();
/// #   fn id(&self) -> WindowId { WindowId(1) }
/// #   fn request_redraw(&self) {}
/// #   fn surface_target(&self) -> Arc<dyn SurfaceTarget> { Arc::new(NoNativeSurface) }
/// #   fn redraw_requester(&self) -> Arc<dyn RedrawRequester> { Arc::new(|| {}) }
/// #   fn state(&self) -> WindowState {
/// #       WindowState::new(WindowGeometry::new(None, Size::default()), Size::default())
/// #   }
/// #   fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
/// #       command.against(&WindowCapabilities::NONE)?;
/// #       Ok(())
/// #   }
/// #   fn set_cursor(&self, _: CursorIcon) -> Result<(), CursorError> { Ok(()) }
/// #   fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> { None }
/// #   fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> { None }
/// #   fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> { None }
/// #   fn drag_source(&self) -> Option<Arc<dyn drag::PlatformDragSource>> { None }
/// }
///
/// let refused = Double.set_frame_rate_hint(FrameRateHint::High).unwrap_err();
/// assert_eq!(refused.reason, UnsupportedReason::NotOnThisPlatform);
/// assert!(Double.set_input_regions(&[]).is_err());
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
