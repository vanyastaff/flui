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
//! - [`HitTestBehavior`]: Controls how hit testing proceeds
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's hit testing system in `gestures/hit_test.dart`.
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

mod behavior;
mod entry;
mod result;
mod target;
mod transform;

pub use behavior::HitTestBehavior;
pub use entry::{BoxHitTestEntry, HitTestEntry, SliverHitTestEntry};
pub use result::{BoxHitTestResult, HitTestResult, SliverHitTestResult};
pub use target::{HitTestTarget, PointerDeviceKind, PointerEvent, PointerEventKind};
pub use transform::MatrixTransformPart;
