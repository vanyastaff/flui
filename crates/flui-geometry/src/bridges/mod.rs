//! Interop bridges between flui-geometry's typed primitives and external math
//! crates, each gated behind its own feature so the dependency is opt-in.
//!
//! Currently:
//! - [`kurbo`] (`feature = "kurbo"`) — f64 Bézier/curve interop for the painting
//!   layer and the future Vello raster backend (N-geom PR 3, U8).

#[cfg(feature = "kurbo")]
pub mod kurbo;

#[cfg(feature = "kurbo")]
pub use kurbo::KurboBridgeError;
