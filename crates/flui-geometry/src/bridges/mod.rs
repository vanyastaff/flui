//! Conversion bridges between flui's typed geometry and foreign math libraries.
//!
//! Each bridge lives behind its own Cargo feature so a consumer only pulls the
//! foreign dependency it actually uses. The bridges follow the U14 (Option D)
//! policy: flui owns unit-typed wrappers for polish discipline; foreign
//! libraries (`kurbo`, …) handle their specialized math.
//!
//! # Conversion direction discipline
//!
//! flui geometry is `f32`-backed; the foreign libraries here are `f64`-backed.
//!
//! - **flui → foreign** is lossless widening (`f32` → `f64`) and is exposed as
//!   `From`.
//! - **foreign → flui** is fallible narrowing (`f64` → `f32`) and is exposed as
//!   `TryFrom`, erroring with [`kurbo::KurboBridgeError::OutOfRange`] when a
//!   finite source value does not fit in `f32`.

#[cfg(feature = "kurbo")]
pub mod kurbo;

#[cfg(feature = "kurbo")]
pub use kurbo::KurboBridgeError;
