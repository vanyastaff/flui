//! The window area: the per-window contract
//! [`PlatformWindow`](crate::PlatformWindow) and the window vocabulary, its
//! identity and creation options, the [`WindowState`](crate::WindowState)
//! snapshot, the [`WindowCommand`](crate::WindowCommand)s it takes and the
//! [`WindowCapabilities`](crate::WindowCapabilities) that say which
//! (ADR-0237), placement, geometry, frame and insets, the
//! [`WindowHandler`](crate::WindowHandler) a window is opened with
//! (ADR-0254 §4), the [`SurfaceTarget`](crate::SurfaceTarget) a renderer
//! draws into, the cursor, the brightness and the frames a host asks for.
//!
//! Those names are at the crate root. What one operating system's windows
//! are created with is in [`os_options`] (ADR-0254 §8).

pub(crate) mod brightness;
pub(crate) mod capabilities;
pub(crate) mod cursor;
pub(crate) mod frame;
pub(crate) mod frame_request;
pub(crate) mod geometry;
pub(crate) mod handler;
pub(crate) mod insets;
pub(crate) mod options;
pub mod os_options;
pub(crate) mod placement;
pub(crate) mod platform_window;
pub(crate) mod state;
pub(crate) mod surface_target;
