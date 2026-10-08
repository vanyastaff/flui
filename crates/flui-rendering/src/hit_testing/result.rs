//! Canonical hit-test result re-exported from `flui_interaction::routing`.
//!
//! Rendering protocols accumulate the same data-only hit path that the
//! presentation's interaction lane resolves. The result owns the transform
//! stack and entries; executable dispatch remains with the presentation.

pub use flui_interaction::routing::HitTestResult;
