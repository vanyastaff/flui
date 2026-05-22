//! Hit testing infrastructure for pointer interaction.
//!
//! This module provides the complete hit testing system used to determine
//! which render objects are located at a given position. This is essential
//! for handling pointer events like taps, drags, and hovers.
//!
//! # Key Types
//!
//! - [`HitTestResult`]: Accumulates hit test entries during traversal
//! - [`HitTestEntry`]: An entry in the hit test path
//! - [`HitTestTarget`]: Trait for objects that can handle hit test events
//! - [`HitTestBehavior`]: Controls how hit testing proceeds (from
//!   flui_interaction)
//!
//! # Protocol-Specific Types
//!
//! - [`BoxHitTestResult`], [`BoxHitTestEntry`]: For RenderBox hit testing
//! - [`SliverHitTestResult`], [`SliverHitTestEntry`]: For RenderSliver hit
//!   testing
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's hit testing system in
//! `gestures/hit_test.dart`.
//!
//! # Example
//!
//! ```ignore
//! let mut result = HitTestResult::new();
//! render_view.hit_test(&mut result, Offset::new(100.0, 200.0));
//!
//! // Process hit targets (front to back)
//! for entry in result.path() {
//!     if let Some(target) = entry.target.upgrade() {
//!         target.handle_event(event, entry);
//!     }
//! }
//! ```

mod entry;
mod result;
mod target;
mod transform;

// Re-export HitTestBehavior from flui_interaction (base type).
// Cycle 4 U-3: parallel `BoxHitTestEntry`/`SliverHitTestEntry` (from
// `entry.rs`) and `BoxHitTestResult`/`SliverHitTestResult` (from
// `result.rs`) were deleted; the protocol-canonical versions live in
// `crates/flui-rendering/src/protocol/box_protocol.rs` and
// `crates/flui-rendering/src/protocol/sliver_protocol.rs`. The
// trait-dispatch `HitTestEntry` + `HitTestResult` types remain here
// and are U-4's migration target.
pub use entry::HitTestEntry;
pub use flui_interaction::routing::HitTestBehavior;
pub use result::HitTestResult;
pub use target::{HitTestTarget, PointerDeviceKind, PointerEvent, PointerEventKind};
pub use transform::MatrixTransformPart;
