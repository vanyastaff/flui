//! Hit testing infrastructure for pointer interaction.
//!
//! After cycle 4 R-7 + R-8 + R-9 consolidation (Wave 2), this module
//! is a thin protocol-extension surface over
//! `flui_interaction::routing` and `flui_interaction::mouse_tracker`.
//! The canonical `HitTestResult` / `HitTestEntry` / `HitTestBehavior`
//! types live in `flui-interaction` (Flutter's `gestures/` ↔
//! `flui-interaction`); this module re-exports them for caller
//! convenience and owns the rendering-protocol-specific
//! `MatrixTransformPart` helper.
//!
//! # Key Types
//!
//! - [`HitTestResult`]: re-exported from
//!   [`flui_interaction::routing::HitTestResult`] -- canonical
//!   result with `Vec<HitTestEntry>` + transform stack + handler
//!   dispatch.
//! - [`HitTestEntry`]: re-exported from
//!   [`flui_interaction::routing::HitTestEntry`] -- carries
//!   `target: RenderId`, optional handler/scroll_handler/cursor.
//! - [`HitTestBehavior`]: re-exported from
//!   [`flui_interaction::routing::HitTestBehavior`] -- standard
//!   `DeferToChild` / `Opaque` / `Translucent` enum.
//! - [`MatrixTransformPart`]: protocol-specific transform helper
//!   used by the box/sliver hit-test capability types.
//!
//! # Protocol-Specific Types
//!
//! `BoxHitTestResult` / `BoxHitTestEntry` / `SliverHitTestResult` /
//! `SliverHitTestEntry` are in `crate::protocol` (next to the
//! `BoxProtocol` / `SliverProtocol` capability definitions). They
//! lived here pre-cycle as parallels to the protocol-side versions,
//! deleted in cycle 4 U-3.
//!
//! # Cycle 4 deletions (Wave 2)
//!
//! U-3 removed the parallel `BoxHitTestEntry` / `BoxHitTestResult` /
//! `SliverHitTestEntry` / `SliverHitTestResult` structs.
//!
//! U-4 removed the rendering-side `HitTestResult` + `HitTestEntry`,
//! replacing them with re-exports of the canonical interaction-side
//! types.
//!
//! U-5 removed the entire `target.rs` module: the `HitTestTarget`
//! trait, `PointerEvent`, `PointerDeviceKind`, and `PointerEventKind`.
//! The trait had one production impl (`RenderView`, deleted in U-4)
//! plus two file-private `DummyTarget` stubs (deleted in U-3/U-4
//! alongside the structs that owned them). No remaining workspace
//! consumers.
//!
//! # Flutter Equivalence
//!
//! Mirrors Flutter's hit-testing split: `gestures/hit_test.dart`
//! owns the base types (now `flui-interaction`), `rendering/box.dart`
//! and `rendering/sliver.dart` own the protocol-specific wrappers
//! (`crate::protocol`).
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::hit_testing::HitTestResult;
//! use flui_types::Offset;
//!
//! let mut result = HitTestResult::new();
//! pipeline_owner.hit_test(&mut result, Offset::new(100.0, 200.0));
//!
//! // Dispatch handlers attached to entries during traversal.
//! result.dispatch(&pointer_event);
//! ```

mod entry;
mod result;
mod transform;

// Canonical types re-exported from flui-interaction. Cycle 4 U-4
// replaced the in-crate `HitTestEntry` + `HitTestResult` structs
// with these re-exports; consumers' `use crate::hit_testing::HitTestResult`
// imports compile unchanged.
pub use entry::HitTestEntry;
pub use flui_interaction::routing::HitTestBehavior;
// Pointer-event dispatch surface: a `RenderObject` advertises a
// `PointerEventHandler` (see `RenderObject::pointer_event_handler`) that the
// pipeline attaches to its hit entry; `HitTestResult::dispatch` then invokes it
// with a `PointerEvent`, honoring the returned `EventPropagation`.
pub use flui_interaction::events::PointerEvent;
pub use flui_interaction::routing::{EventPropagation, PointerEventHandler};
pub use result::HitTestResult;
pub use transform::MatrixTransformPart;
