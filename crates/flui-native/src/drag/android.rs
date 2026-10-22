//! Android drag and drop through `FluiView` (ADR-0211, ADR-0247).
//!
//! - [`view_model`]: what a `DragEvent`'s clip offers, what a drop holds and
//!   answers, what a FLUI drag puts in its `ClipData` and how its source learns
//!   the end, decided without JNI; every host checks it;
//! - `view`: the JNI side on Android — the transport the view's drag
//!   callbacks reach, and a window's drag source.

mod view_model;

#[cfg(target_os = "android")]
mod view;

#[cfg(target_os = "android")]
pub(crate) use view::{AndroidDragSource, AndroidTransfer};
