//! Hit testing infrastructure for pointer interaction.
//!
//! This module is a thin protocol-extension surface over
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
//!   `target: RenderId`, optional pointer_target/scroll_handler/cursor.
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
//! `BoxProtocol` / `SliverProtocol` capability definitions). This
//! module used to carry its own parallel copies of those types; the
//! duplicates were removed since the protocol-side versions are the
//! ones the box/sliver capability code actually consumes.
//!
//! # What this module used to contain
//!
//! This module previously defined its own `HitTestResult` and
//! `HitTestEntry` structs, separate from the canonical interaction-side
//! versions; those were removed and replaced with the re-exports above
//! so there is only one hit-test result type in the workspace, not two
//! that could drift apart.
//!
//! It also used to contain an entire `target.rs` module: a
//! `HitTestTarget` trait plus `PointerEvent`, `PointerDeviceKind`, and
//! `PointerEventKind` types. That module was deleted once it had no
//! remaining purpose: the trait had exactly one production
//! implementation (on `RenderView`, itself removed once the rendering
//! side stopped defining its own `HitTestResult`), plus two
//! file-private `DummyTarget` test stubs that went away with the
//! structs they supported. With no implementors or consumers left in
//! the workspace, the trait and its module were removed rather than
//! kept as dead code.
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

// Canonical types re-exported from flui-interaction. These re-exports
// replaced the in-crate `HitTestEntry` + `HitTestResult` structs, so
// consumers' `use crate::hit_testing::HitTestResult` imports compile
// unchanged.
pub use entry::HitTestEntry;
pub use flui_interaction::routing::HitTestBehavior;
// Pointer-event dispatch surface: a `RenderObject` advertises a data-only
// `PointerTarget` (see `RenderObject::pointer_target`) that the pipeline
// attaches to its hit entry; dispatch resolves the target through the
// owner-local interaction lane and delivers the locally transformed
// `PointerEvent` leaf-first to every target (ADR-0027 — executable callbacks
// never live in render storage). `EventPropagation` remains scroll-only.
pub use flui_interaction::events::{CursorIcon, InputEvent, PointerEvent, PointerEventExt};
pub use flui_interaction::routing::{
    DeviceId, EventPropagation, MouseEnterCallback, MouseExitCallback, MouseHoverCallback,
    MouseRegionCallbacks, MouseRegionTarget, MouseTrackerAnnotation, PathClipTarget, PointerTarget,
    resolve_path_clip_target,
};
pub use result::HitTestResult;
pub use transform::MatrixTransformPart;
