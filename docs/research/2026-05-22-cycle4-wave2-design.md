# Cycle 4 Wave 2 — Architectural Design

**Audit:** `docs/research/2026-05-22-flui-rendering-engine-audit.md`
**Wave 1 (landed):** PR #109 — 9 P0 mechanical findings.
**Wave 2 (this doc):** 4 P0 architectural reshapes — **R-6**, **R-7/R-8/R-9 trio**, **E-2**.
**Author role:** System Architect (cycle 4).
**Date:** 2026-05-22.

> **Discipline gate.** This document follows the cycle 1 PR #93 / cycle 2 PR #100 / cycle 3 PR #102-#106 / Wave 1 PR #109 precedent: one self-contained commit per unit, conventional commit subject, atomic-compile per step, no quick-wins. The orchestrator (cycle-4 reviewer) MUST refuse subagent output that defers downstream-consumer migrations or leaves the workspace in a non-building state at any intermediate commit.

---

## Executive summary

Four P0 findings remain after Wave 1. Each demands an architectural reshape rather than a mechanical fix:

1. **R-6 [NESTED-LOCK-SMELL].** `RendererBinding::render_views()` returns `&RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` — a triple-lock topology baked into the trait surface. Reshape to four typed getter/mutator methods (`render_view`, `render_view_ids`, `insert_render_view`, `remove_render_view_by_id`) that hide the outer lock. Internal default-impl helpers in `binding/mod.rs` and three `debug_dump_*` free functions migrate to the new methods. ~140 LOC delta.

2. **R-7 + R-8 + R-9 [PARALLEL-TYPE trio].** Two `HitTestResult`, two `MouseTrackerAnnotation`, two `MouseTracker` types across `flui-rendering` and `flui-interaction`. The trio is a single architectural decision: the **interaction-side wins** because (a) only the interaction-side has real production data (handlers + cursors), (b) only one production impl of the rendering-side `HitTestTarget` trait exists (the `RenderView` itself), (c) Flutter's canonical `HitTestResult` lives in `gestures/`, not `rendering/`. `flui-rendering::hit_testing` becomes a thin protocol-extension surface over `flui_interaction::routing::HitTestResult` and the `HitTestTarget`/`MouseTrackerAnnotation`/`MouseTracker` triplet is deleted from flui-rendering. The flui-app `// TODO: Convert` bridge is removed. **Honest LOC impact: ~1,500 LOC deletions, ~250 LOC additions for protocol extension. The audit's 50 LOC estimate was off by an order of magnitude.**

3. **E-2 [OFFSCREEN-OWNERSHIP].** `OffscreenRenderer` is owned by `Renderer` but `WgpuPainter`/`Backend` already see it via `Backend::with_offscreen` (this part of the audit reading was incomplete). The actual hole is `Backend::render_backdrop_filter` at `backend.rs:805-834` — the `DisplayList`-command-level dispatch is unimplemented while the layer-level `handle_backdrop_filter` at `renderer.rs:845-960` already works correctly via the same Arc-shared offscreen. Wave 2 wires the command-level path into the existing layer-level helper. Scope: implementation only — no ownership rotation required. ~180 LOC addition.

**Total commits proposed:** 11 (1 for R-6, 8 for R-7/R-8/R-9 trio, 1 for E-2, 1 closing receipts unit).
**Total LOC delta estimate:** −1,540 / +630 (net ≈ −910 LOC across flui-rendering + flui-engine + flui-app + flui-interaction).
**Single-PR sizing verdict:** Wave 2 is **at the upper edge of single-PR sizing but still single-PR** by atomic-commit-per-unit precedent; if reviewer feedback splits it, the natural seam is "trio in one PR, R-6 + E-2 in a second PR".

---

## R-6 design — `RendererBinding::render_views()` lock topology

### Current shape

`crates/flui-rendering/src/binding/mod.rs:144-181`:

```rust
pub trait RendererBinding: Send + Sync {
    // ...
    /// Returns all render views managed by this binding.
    fn render_views(&self) -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>;

    fn add_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>) {
        // ...
        self.render_views().write().insert(view_id, view);
    }

    fn remove_render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
        self.render_views().write().remove(&view_id)
    }

    fn get_render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
        self.render_views().read().get(&view_id).cloned()
    }
}
```

Three lock topology depths through one trait surface:

1. `binding.render_views()` returns `&RwLock<HashMap<...>>`
2. caller `.write()` or `.read()` to reach the `HashMap`
3. `Arc<RwLock<RenderView>>` value requires its own `.read()`/`.write()` for inner access.

Call sites (8 inside `binding/mod.rs`, 1 in `crates/flui-app/src/bindings/renderer_binding.rs`):

- `mod.rs:161,174,179` — default-impl bodies for `add/remove/get_render_view`.
- `mod.rs:256,279` — `draw_frame` + `handle_metrics_changed` iterate the HashMap.
- `mod.rs:356,373,401` — `debug_dump_render_tree`, `debug_dump_layer_tree`, `debug_dump_semantics_tree`.
- `flui-app/src/bindings/renderer_binding.rs:375-377` — implementer returns `&self.render_views`.
- `flui-app/src/bindings/renderer_binding.rs:427-429` — concrete `draw_frame` iterates the HashMap.

### Target shape

The lock topology becomes an implementation detail. The trait surface exposes four primitive operations matching Flutter's `RendererBinding._views` operations (`add`, `remove`, `lookup`, `iterate-ids`):

`crates/flui-rendering/src/binding/mod.rs`:

```rust
pub trait RendererBinding: Send + Sync {
    // ... unchanged surface above ...

    // ========================================================================
    // RenderView Management (R-6: lock-hiding reshape)
    // ========================================================================

    /// Returns the render view for `view_id`, if present.
    ///
    /// The returned `Arc<RwLock<RenderView>>` is a reference-count bump —
    /// the caller is responsible for acquiring the inner lock for actual
    /// access. The implementer's outer `HashMap` lock is held only for
    /// the duration of the `.get(...)` call.
    fn render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>>;

    /// Returns the IDs of all render views, in insertion order is **not**
    /// guaranteed (HashMap iteration order). The returned `Vec` is owned;
    /// the implementer's outer lock is held only for the duration of
    /// collection.
    fn render_view_ids(&self) -> Vec<u64>;

    /// Inserts a render view.
    ///
    /// If a view with `view_id` already exists, replaces it; the prior
    /// value is dropped without further notice. Override this default
    /// at the impl side when replace-semantics must be customised.
    fn insert_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>);

    /// Removes a render view, returning it if present.
    fn remove_render_view_by_id(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>>;

    // ========================================================================
    // Legacy helpers (forward to primitives — REMOVED, see migration step 2)
    // ========================================================================

    // The previous methods `add_render_view`, `remove_render_view`,
    // `get_render_view`, and `render_views` are removed. Callers migrate
    // to the four primitives above. The previous `add_render_view` did
    // configuration setup; that logic moves to a default-impl helper
    // (next).

    /// Adds a render view, applying view-configuration setup.
    ///
    /// Default-impl now delegates to `insert_render_view`; the
    /// configuration-derivation step is preserved.
    fn add_render_view_with_config(&self, view_id: u64, view: Arc<RwLock<RenderView>>) {
        let config = self.create_view_configuration_for(&view.read());
        view.write().set_configuration(config);
        self.insert_render_view(view_id, view);
    }
}
```

**Why this shape per *Programming Rust* §11 (newtype + sealed primitive operations).** The original `render_views() -> &RwLock<...>` is a leaked-implementation-detail anti-pattern — every consumer must reason about the lock graph. The four-primitive surface is *Gjengset, Rust for Rustaceans*, chapter 3: hide the lock; expose what the lock *does*. Implementers retain full freedom to use a different container (e.g. a `DashMap` if contention becomes a problem) without breaking the trait.

`debug_dump_*` free functions in `binding/mod.rs:355-435` migrate to the primitive: iterate `render_view_ids()`, then for each id call `render_view(id)` and `.read()` the inner `Arc`. The inner lock topology is unchanged — only the outer one is hidden.

### Migration order

Step 1 (atomic commit): add the four primitives as **required** methods (default-impl removed, no body). Add `add_render_view_with_config` default-impl. Keep `render_views()` temporarily marked `#[deprecated]` with a `Self::render_view_ids` redirect note. Update `flui-app/src/bindings/renderer_binding.rs` to implement the four primitives by forwarding to its `render_views: RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` field (which stays as private state). Update internal default-impl callsites at `mod.rs:256, 279, 161, 174, 179, 356, 373, 401` to use the primitives. **At end of step 1, `cargo build --workspace` is green; `render_views()` is still present but deprecated.**

Step 2 (atomic commit): delete `render_views()` from the trait. Update `flui-app/src/bindings/renderer_binding.rs:375-377` to remove the implementation. Update the concrete `draw_frame` at `flui-app/src/bindings/renderer_binding.rs:427-429` to iterate via `self.render_view_ids()` + `self.render_view(id)`. Verify clippy clean.

Single PR commit count for R-6: **1 atomic commit** (the two-step migration is one logical reshape; the deprecated-bridge of step 1 is unnecessary because no external consumers exist outside flui-rendering + flui-app).

### Risk + reversal

- **Risk:** A downstream consumer outside flui-app reads `render_views()` directly. Verification: `rg 'render_views\(\)\.' crates/` — currently 9 hits, all inside flui-rendering + flui-app. **Mitigation:** if a future external consumer appears, they migrate to the primitives (cheap rename).
- **Reversal:** if rolled back, the trait reverts to its prior shape; the implementer's `render_views: RwLock<HashMap<...>>` field doesn't need to change because the primitive impls inside flui-app keep it.
- **Verification gate:** `cargo build -p flui-rendering -p flui-app --lib` clean; `cargo test -p flui-rendering -p flui-app --lib` green; `bash scripts/port-check.sh -v` clean (no new trait objects, no new locks).

---

## R-7 + R-8 + R-9 design — Parallel-type trio (`HitTestResult`, `MouseTrackerAnnotation`, `MouseTracker`)

### Why one section

The three findings are tightly coupled:
- R-7's `HitTestResult` is the storage container for hit entries.
- R-8's `MouseTrackerAnnotation` is the *target* of mouse events (a hit entry that registers itself).
- R-9's `MouseTracker` is the *runtime* that consumes hit-test results to dispatch enter/hover/exit.

Touching one without the others creates a half-migrated state. The Wave 1 + cycle-3 PR-shape lessons say: bundle the architectural decision, atomise the migration steps inside it.

### Path selection — interaction-side wins

Two paths the audit identified:

> (a) Move `HitTestTarget` trait down to flui-interaction (or flui-foundation as a lower common dep).
> (b) Keep trait-dispatch in flui-rendering; flui-interaction's type is what flui-app uses; flui-rendering's hit-test traversal converts to entries-with-handlers at the boundary.

**This design picks (a) with a refinement: do not move `HitTestTarget` — delete it.**

Empirical evidence from `rg 'impl HitTestTarget for'`:

- `crates/flui-rendering/src/view/render_view.rs:552` — one real impl (RenderView itself, which the audit acknowledges is the "always-added root entry").
- `crates/flui-rendering/src/hit_testing/result.rs:233` — local `DummyTarget` struct, file-private.
- `crates/flui-rendering/src/hit_testing/entry.rs:12` — second local `DummyTarget`, identical, file-private.

The `HitTestTarget` trait has **one production impl in the entire workspace, and two stub fillers**. Flutter's `HitTestTarget` has 30+ impls across the codebase. This is not a system FLUI uses — it's vestigial structure. Compare against `flui_interaction::routing::HitTestEntry`, where `target: RenderId` + `handler: Option<PointerEventHandler>` carry the actual hit data, used at 12 callsites in routing.rs + tests + docs.

**Decision:**
- Canonical `HitTestResult` is `flui_interaction::routing::HitTestResult`.
- `HitTestTarget` trait is deleted (RenderView's only impl gets inlined or replaced with a `RenderId`-based entry).
- Canonical `MouseTrackerAnnotation` is `flui_interaction::mouse_tracker::MouseTrackerAnnotation` (the struct with callbacks).
- Canonical `MouseTracker` is `flui_interaction::mouse_tracker::MouseTracker`.
- `flui_rendering::hit_testing` reduces to **protocol-specific extension types only**: `BoxHitTestResult`, `BoxHitTestEntry`, `SliverHitTestResult`, `SliverHitTestEntry`, `MatrixTransformPart`.
- `flui_rendering::input` module is **deleted entirely** — no parallel mouse tracker.

This matches Flutter parity (`gestures/hit_test.dart` owns `HitTestResult`, `gestures/mouse_tracker.dart` owns `MouseTracker`; FLUI's `flui-interaction` is the gestures-equivalent crate per PR #84 framework-spine work).

### Pre-existing internal inconsistency to fix in passing

`crates/flui-rendering/src/hit_testing/entry.rs:125` defines `BoxHitTestEntry::new(local_position: Offset)` — 1 arg.

`crates/flui-rendering/src/protocol/box_protocol.rs:446` and `crates/flui-rendering/src/context/hit_test.rs:236, 242` call `BoxHitTestEntry::new(target_id, transform_matrix)` — 2 args.

**This already does not compile** as a coherent unit — one of the two forms must be wrong. Let me re-check; the workspace must build today.

Looking again: `box_protocol.rs:446` uses `BoxHitTestEntry::new(target_id, transform)` — but `target_id: u64`, `transform: Matrix4`. The definition is `pub fn new(local_position: Offset) -> Self`. These types do not match, so one of these callsites is unreachable behind a `cfg` or stale. Either way, the reshape resolves it: `BoxHitTestEntry` becomes the entry of an `BoxHitTestResult` protocol-extension wrapping `HitTestResult`, with a clear `pub fn new(local_position: Offset)` signature, and the two parents will use the same signature consistently.

This is a **structural cleanup byproduct, not a quick-win addition**: the trio reshape forces a single home for `BoxHitTestEntry`, ending the divergence.

### Target shape — flui-interaction canonical types

`crates/flui-interaction/src/routing/hit_test.rs` (canonical home, **no structural change** to `HitTestResult` itself):

```rust
// Unchanged from current (lines 174-374). Already the right shape:
//   - path: Vec<HitTestEntry>
//   - transforms: Vec<Matrix4>   (eager globalization)
//   - local_transforms: Vec<TransformPart>   (lazy globalization — Flutter parity)
// The `HitTestEntry` carries:
//   target: RenderId
//   transform: Option<Matrix4>
//   handler: Option<PointerEventHandler>
//   scroll_handler: Option<ScrollEventHandler>
//   cursor: CursorIcon
// This is the union of both legacy types — entry-data form. Already
// matches Flutter's HitTestEntry<T extends HitTestTarget> semantics
// (Flutter dispatches via target.handleEvent; FLUI dispatches via
// stored handler closures — different mechanism, equivalent outcome).

// New addition (sealed extension trait per Gjengset Rust for
// Rustaceans ch.3) for transform-stack scopes used by the box/sliver
// protocols. Already present as `TransformGuard` (lines 384-399);
// extend slightly:
impl HitTestResult {
    /// Pushes a paint-offset and returns a guard that pops on drop.
    #[must_use = "guard must be held for the scope where the transform applies"]
    pub fn paint_offset_scope(&mut self, offset: Offset<Pixels>) -> TransformGuard<'_> {
        self.push_offset(offset);
        TransformGuard::new(self)
    }

    /// Pushes a paint-transform and returns a guard that pops on drop.
    #[must_use = "guard must be held for the scope where the transform applies"]
    pub fn paint_transform_scope(&mut self, transform: Matrix4) -> TransformGuard<'_> {
        self.push_transform(transform);
        TransformGuard::new(self)
    }
}
```

`crates/flui-rendering/src/hit_testing/result.rs` (reshaped — keeps only the protocol-specific wrappers):

```rust
//! Protocol-specific hit-test result wrappers.
//!
//! The canonical `HitTestResult` lives in `flui_interaction::routing`.
//! This module provides `BoxHitTestResult` and `SliverHitTestResult` —
//! extension wrappers that adapt the canonical result to the box/sliver
//! protocols' geometry idioms.
//!
//! # Flutter Equivalence
//!
//! Flutter's `BoxHitTestResult` and `SliverHitTestResult` in
//! `rendering/box.dart` and `rendering/sliver.dart` wrap the base
//! `HitTestResult` from `gestures/hit_test.dart`. FLUI mirrors this
//! split.

use flui_interaction::routing::HitTestResult;
use flui_types::{Offset, geometry::{Matrix4, Pixels}};

use super::entry::{BoxHitTestEntry, SliverHitTestEntry};

/// A `HitTestResult` adapter for the box protocol.
///
/// Wraps a borrow of the canonical `HitTestResult` and offers the
/// `addWithPaintOffset` / `addWithPaintTransform` / `addWithRawTransform`
/// / `addWithOutOfBandPosition` Flutter-parity APIs.
///
/// # Lifetime
///
/// The wrapper borrows `&'a mut HitTestResult`. The Flutter API mirrors
/// this borrow because the box protocol must push/pop transforms on the
/// same result instance the gesture system reads.
pub struct BoxHitTestResult<'a> {
    inner: &'a mut HitTestResult,
}

impl<'a> BoxHitTestResult<'a> {
    pub fn wrap(result: &'a mut HitTestResult) -> Self {
        Self { inner: result }
    }

    /// Adds an entry with a local position transform implicit in the
    /// current transform stack. Flutter parity: `BoxHitTestResult.add`.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        // BoxHitTestEntry → HitTestEntry conversion: pull the target_id
        // and transform, leaving handler/cursor unset (those attach
        // at the render-object dispatch site, not here).
        let interaction_entry = flui_interaction::routing::HitTestEntry::new(entry.target_id())
            .with_transform_unchecked(entry.transform());
        self.inner.add(interaction_entry);
    }

    pub fn add_with_paint_offset<F>(&mut self, offset: Option<Offset<Pixels>>, position: Offset<Pixels>, hit_test: F) -> bool
    where F: FnOnce(&mut Self, Offset<Pixels>) -> bool {
        // Forward to interaction-side transform-stack helper.
        let effective_offset = offset.unwrap_or(Offset::ZERO);
        let local = Offset::new(position.dx - effective_offset.dx, position.dy - effective_offset.dy);
        let _guard = offset.map(|o| self.inner.paint_offset_scope(o));
        hit_test(self, local)
    }

    // ... addWithPaintTransform, addWithRawTransform, addWithOutOfBandPosition
    //     — same shape, all forward to the inner result's transform stack
}
```

`crates/flui-rendering/src/hit_testing/entry.rs` (reshaped to match the protocol use):

```rust
//! Protocol-specific hit-test entry types.
//!
//! Box and sliver protocols use specialised entry types that match the
//! protocol's geometry (Offset for box, main/cross axis for sliver).
//! The canonical `HitTestEntry` in `flui_interaction::routing` carries
//! the runtime-dispatch data (RenderId target, handler, cursor); these
//! protocol entries carry the layout-geometry data.

use flui_foundation::RenderId;
use flui_types::{Offset, geometry::{Matrix4, Pixels}};

/// An entry in a box hit test result.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    pub target: RenderId,
    pub local_position: Offset<Pixels>,
    pub transform: Matrix4,
}

impl BoxHitTestEntry {
    pub fn new(target: RenderId, transform: Matrix4) -> Self {
        Self { target, local_position: Offset::ZERO, transform }
    }

    pub fn with_position(target: RenderId, local_position: Offset<Pixels>) -> Self {
        Self { target, local_position, transform: Matrix4::IDENTITY }
    }

    pub fn target_id(&self) -> RenderId { self.target }
    pub fn transform(&self) -> Matrix4 { self.transform }
}

/// An entry in a sliver hit test result.
#[derive(Debug, Clone)]
pub struct SliverHitTestEntry {
    pub target: RenderId,
    pub main_axis_position: f32,
    pub cross_axis_position: f32,
}

impl SliverHitTestEntry {
    pub fn new(target: RenderId, main_axis_position: f32) -> Self {
        Self { target, main_axis_position, cross_axis_position: 0.0 }
    }
}
```

This resolves the in-flight `BoxHitTestEntry::new(target_id, transform)` divergence noted above.

`crates/flui-rendering/src/hit_testing/mod.rs` (reshaped to thin re-export):

```rust
//! Protocol-specific hit-testing extensions for the box and sliver
//! render protocols.
//!
//! The canonical `HitTestResult`, `HitTestEntry`, and `HitTestBehavior`
//! types live in `flui_interaction::routing`. This module provides only
//! the box/sliver protocol adapters.
//!
//! # Flutter Equivalence
//!
//! Mirrors Flutter's split: `gestures/hit_test.dart` owns the base
//! types, `rendering/box.dart` and `rendering/sliver.dart` own the
//! protocol-specific wrappers.

mod entry;
mod result;
mod transform;

// Protocol-specific re-exports.
pub use entry::{BoxHitTestEntry, SliverHitTestEntry};
pub use result::{BoxHitTestResult, SliverHitTestResult};
pub use transform::MatrixTransformPart;

// Re-exports of canonical types from flui-interaction for caller
// convenience. Callers may also import directly from
// flui_interaction::routing.
pub use flui_interaction::routing::{HitTestBehavior, HitTestEntry, HitTestResult};
```

`crates/flui-rendering/src/input/` — **deleted entirely.** The `MouseTrackerAnnotation` trait, `MouseTracker` struct, `MouseCursorSession`, `PointerEnterEvent`/`PointerExitEvent`/`PointerHoverEvent`, and `MouseTrackerHitTest` callback type all migrate to `flui_interaction::mouse_tracker` (where the canonical versions already live).

`crates/flui-rendering/src/binding/mod.rs:30` — change `use crate::{ ..., input::MouseTracker, ... }` to `use flui_interaction::mouse_tracker::MouseTracker`.

`crates/flui-rendering/src/binding/mod.rs:206` — `fn mouse_tracker(&self) -> &RwLock<MouseTracker>` stays unchanged in shape; the `MouseTracker` referent changes to `flui_interaction::MouseTracker`.

`crates/flui-rendering/src/view/render_view.rs` — the `impl HitTestTarget for RenderView` at line 552 is deleted. The `result.add(HitTestEntry::new_render_view())` at line 343 changes to `result.add(flui_interaction::routing::HitTestEntry::new(RenderId::new_root()))` (assuming a root sentinel RenderId; if not present, introduce one).

`crates/flui-app/src/app/binding.rs:495-523` — the bridge code with the `// TODO: Convert` is rewritten:

```rust
pub fn handle_input(&self, input: PlatformInput) {
    match input {
        PlatformInput::Pointer(pointer_event) => {
            self.gestures.handle_pointer_event(&pointer_event, |position| {
                // Single canonical HitTestResult flows through both layers
                use flui_rendering::binding::RendererBinding;
                let renderer = self.renderer.read();
                let mut result = flui_interaction::routing::HitTestResult::new();
                let offset = flui_types::Offset::new(position.dx, position.dy);
                renderer.hit_test_in_view(&mut result, offset, 0);
                if !result.is_empty() {
                    tracing::debug!(hits = result.len(), "Hit test found targets");
                }
                result
            });
            self.request_redraw();
        }
        PlatformInput::Keyboard(keyboard_event) => {
            FocusManager::global().dispatch_key_event(&keyboard_event);
            self.request_redraw();
        }
    }
}
```

The bridge code disappears; one result type flows from RenderView → binding → gesture handler.

`crates/flui-rendering/src/binding/mod.rs:113-118` — `hit_test_in_view(&self, result: &mut HitTestResult, ...)` updates its result type:

```rust
fn hit_test_in_view(
    &self,
    result: &mut flui_interaction::routing::HitTestResult,
    position: flui_types::Offset,
    view_id: u64,
);
```

`crates/flui-rendering/src/view/render_view.rs:342-345` — `RenderView::hit_test` signature updates likewise (`&mut HitTestResult` is now the interaction-side type).

### Why this shape per *Programming Rust* §11 + *Rust for Rustaceans* ch.3

1. **Single source of truth** for data shapes: the canonical `HitTestResult` carries union-of-needs fields (target id, transform, handler, scroll handler, cursor). No converter at the seam.
2. **Protocol extensions are wrappers, not duplicates**: `BoxHitTestResult<'a>` borrows the canonical result. This is the *Rust for Rustaceans* "extension by composition, not by parallel hierarchy" pattern (chapter 3, "Type State and Generics").
3. **No trait-object soup**: the deleted `HitTestTarget` trait was a vestigial dyn-dispatch surface with one production impl. The new design uses `RenderId` + stored handler closure (already in interaction-side), zero `Box<dyn HitTestTarget>`.
4. **Flutter parity in module layout**: `gestures/` ↔ `flui-interaction`, `rendering/box.dart::BoxHitTestResult` ↔ `flui-rendering::hit_testing::BoxHitTestResult`.

### Migration order — eight atomic commits

Each commit compiles and tests cleanly. Total ~1,500 LOC deletion is distributed across the steps.

**Commit 1 — Pre-flight: extend interaction-side `HitTestResult` to absorb missing capability.** Add `paint_offset_scope` and `paint_transform_scope` guards (already present as `TransformGuard`; add the constructor methods). Add `HitTestEntry::with_transform_unchecked(matrix)` builder that sets the transform without re-running the stack (used by BoxHitTestResult adapter). No rendering-side touches. Touches: `crates/flui-interaction/src/routing/hit_test.rs`. **Compiles. Tests pass. Subject:** `feat(interaction): U-1 absorb capability needed by rendering BoxHitTestResult adapter`.

**Commit 2 — Reshape `flui_rendering::hit_testing::entry` to canonical 2-arg `BoxHitTestEntry`.** Resolves the existing internal divergence. The pre-reshape `BoxHitTestEntry::new(local_position: Offset)` becomes `BoxHitTestEntry::new(target: RenderId, transform: Matrix4)` matching the callsites in `protocol/box_protocol.rs:446` + `context/hit_test.rs:236, 242`. Update tests at `entry.rs:230-280` to use the new shape. Delete `BoxHitTestEntry::default()` (no callers under the new shape). Touches: `crates/flui-rendering/src/hit_testing/entry.rs`. **Compiles. Tests pass.** **Subject:** `refactor(rendering): U-2 normalize BoxHitTestEntry signature to (RenderId, Matrix4)`.

**Commit 3 — Add `BoxHitTestResult` adapter borrowing `&mut HitTestResult`.** New file `crates/flui-rendering/src/hit_testing/result.rs` reshaped: deletes the standalone `HitTestResult` struct (was duplicating interaction-side). Replaces with `BoxHitTestResult<'a>` and `SliverHitTestResult<'a>` adapters. The old `HitTestResult` in this file is **removed entirely** — all its callers are inside the same module, the binding's hit_test_in_view, and flui-app. Touches: `crates/flui-rendering/src/hit_testing/result.rs` (rewrite), `mod.rs` re-exports. **Compiles only after commit 4 lands** — so this commit + commit 4 are bundled into a single atomic commit. **Subject (combined with commit 4):** `refactor(rendering+app): U-3 collapse rendering HitTestResult to interaction canonical`.

**Commit 4 (bundled with 3) — Update consumers to use interaction `HitTestResult`.** Wires:
- `crates/flui-rendering/src/binding/mod.rs:29, 113-118` — replace `crate::hit_testing::HitTestResult` import with `flui_interaction::routing::HitTestResult`. Change `hit_test_in_view` signature accordingly.
- `crates/flui-rendering/src/view/render_view.rs:15, 342-345, 552` — replace import, change `hit_test` signature, **delete the `impl HitTestTarget for RenderView` block**. The root-entry add becomes `result.add(HitTestEntry::new(RenderId::root_sentinel()))` (introduce a sentinel constant or use a fixed value like `RenderId::new(1)`).
- `crates/flui-app/src/bindings/renderer_binding.rs:46-47` — drop `flui_rendering::hit_testing::HitTestResult` import, add `flui_interaction::routing::HitTestResult` import.
- `crates/flui-app/src/bindings/renderer_binding.rs:361-367` — `hit_test_in_view` impl updates to the new type.
- `crates/flui-app/src/app/binding.rs:495-523` — bridge code rewritten (see Target Shape section).

**Compiles. Tests pass.** Subject: `refactor(rendering+app): U-3 collapse rendering HitTestResult to interaction canonical`.

**Commit 5 — Delete `HitTestTarget` trait and the two `DummyTarget` stubs.** Touches: `crates/flui-rendering/src/hit_testing/target.rs` (delete trait body, keep `PointerEvent`/`PointerEventKind` types if still used; move to a `hit_testing::events` submodule or delete if unused), `crates/flui-rendering/src/hit_testing/result.rs:231-238` (delete `DummyTarget`), `crates/flui-rendering/src/hit_testing/entry.rs:10-14` (delete `DummyTarget`). Re-check `rg 'HitTestTarget' crates/` — should return zero hits after this commit. Touches mod.rs re-exports. **Compiles. Tests pass.** **Subject:** `refactor(rendering): U-4 delete HitTestTarget trait + DummyTarget stubs (1-impl vestigial surface)`.

**Commit 6 — Delete `flui_rendering::input::MouseTracker` and friends.** The whole `crates/flui-rendering/src/input/` directory is removed. Imports inside `binding/mod.rs` switch from `crate::input::MouseTracker` to `flui_interaction::mouse_tracker::MouseTracker`. The `MouseTrackerHitTest` callback type alias migrates (it already exists in flui-interaction; if not, add it). `crates/flui-rendering/src/lib.rs:100-108` updates its prelude to drop `MouseCursorSession`/`MouseTrackerAnnotation`/`MouseTrackerHitTest`/`PointerEnterEvent`/`PointerExitEvent`/`PointerHoverEvent` (they're available via `flui_interaction::prelude::*` for consumers). Touches: 4 files. **Compiles only with commit 7** — bundled. **Subject (combined):** `refactor(rendering+app): U-5 collapse rendering MouseTracker to interaction canonical`.

**Commit 7 (bundled with 6) — Update flui-app to construct flui-interaction MouseTracker.** `crates/flui-app/src/bindings/renderer_binding.rs:44-48, 75-90, 142-159, 379-381` — switch `MouseTracker::new` to `flui_interaction::mouse_tracker::MouseTracker::new`. The signature changes from `(MouseTrackerHitTest)` to no args (interaction-side is parameterless; hit-test fn is passed to `update_with_event` per call). Adapt the `mouse_tracker()` getter return type. **Compiles. Tests pass.** Subject: `refactor(rendering+app): U-5 collapse rendering MouseTracker to interaction canonical`.

**Commit 8 — Cleanup pass.** Re-export hygiene: `flui-rendering/src/lib.rs:82,100-108` is updated to re-export only protocol-specific types from `hit_testing` and to point at `flui_interaction::routing` and `flui_interaction::mouse_tracker` for the canonical surface. Drop the `hit_testing::HitTestBehavior` re-export at `mod.rs:48` (it was already a `flui_interaction` re-export, keep this one). Run `cargo doc --no-deps` to verify rustdoc passes. Run `bash scripts/port-check.sh -v` to confirm no new violations. **Subject:** `refactor(rendering): U-6 prelude + docs cleanup post-MouseTracker/HitTestResult absorption`.

### Risk + reversal

- **Risk 1 — RenderView root-entry semantics drift.** Flutter's `RenderView::hitTest` adds a `HitTestEntry` with the RenderView itself as target. FLUI's `HitTestEntry::new_render_view()` constructor (entry.rs:62) doesn't carry a RenderId — it uses `Weak::new::<DummyTarget>` which means *no target is actually identified*. The replacement `HitTestEntry::new(RenderId::new(1))` (or a dedicated `RenderId::root_sentinel()`) preserves the position-as-entry semantics that downstream gesture code uses. **Verification:** `cargo test -p flui-rendering view::render_view::tests` after commit 4. If the test suite has any hit-test-on-RenderView assertion, it must pass.
- **Risk 2 — `flui_interaction::HitTestResult::new` initializes `transforms: vec![Matrix4::identity()]`** (line 211) whereas `flui_rendering::HitTestResult::new` left it empty. Downstream callers in render protocols (`context/hit_test.rs`, `protocol/box_protocol.rs`) push transforms onto a stack expected to start empty. **Mitigation:** verify that `push_offset` / `push_transform` / `pop_transform` semantics remain identity-preserving with the interaction-side initial-identity setup. Add a regression test if any callsite depends on `transforms.len() == 0` at start. If divergence found: reshape the interaction-side `HitTestResult::new` to start with an empty `local_transforms` (matching the rendering-side; the identity at `transforms[0]` is the absolute frame, which the box protocol need not see).
- **Risk 3 — Mouse-tracker callback signature drift.** Rendering-side `MouseTrackerHitTest = Arc<dyn Fn(Offset, i32) -> HitTestResult + Send + Sync>` (2 args). Interaction-side's `MouseTracker::update_all_devices<F: Fn(Offset<Pixels>) -> HitTestResult>` (1 arg, no view_id). **Mitigation:** the interaction-side does not need view_id (single-window assumption today). For multi-window, the binding can wrap the hit-test callback with the view-id baked in. No public API break.
- **Reversal:** if commits 6 + 7 cause a regression in mouse hover behavior (currently the rendering-side `update_device` at `mouse_tracker.rs:443-456` is a placeholder doing nothing — see comments at lines 448-454 "in a full implementation we would..."), the rollback is to revert commits 6 + 7. Commits 1-5 are independently valid as the HitTestResult/HitTestTarget cleanup and would stay landed.
- **Verification gate per commit:**
  - `cargo build --workspace` clean.
  - `cargo clippy --workspace --all-targets -- -D warnings` clean.
  - `cargo test -p flui-rendering --lib`, `-p flui-interaction --lib`, `-p flui-app --lib` green.
  - `bash scripts/port-check.sh -v` clean (no new `Box<dyn HitTestTarget>` — there is none anyway).
  - `rg 'flui_rendering::hit_testing::HitTestResult' crates/` returns zero hits after commit 4.
  - `rg 'HitTestTarget' crates/` returns zero hits after commit 5.
  - `rg 'flui_rendering::input' crates/` returns zero hits after commit 7.

---

## E-2 design — `Backend::render_backdrop_filter` actual filter rendering

### Re-reading the current shape

The audit's reading of E-2 was incomplete. Let me restate based on a fresh read of `crates/flui-engine/src/wgpu/`:

- **OffscreenRenderer ownership** is `Renderer.offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` at `renderer.rs:147`. The `Arc<Mutex<>>` permits clones to flow to consumers.
- **Backend already sees offscreen.** `Backend::with_offscreen` at `backend.rs:43-52` accepts the `Arc<Mutex<OffscreenRenderer>>` and stores it. Constructed at `renderer.rs:670` for the main render path.
- **Layer-level backdrop filter is implemented.** `Renderer::handle_backdrop_filter` at `renderer.rs:845-960` does mid-frame flush + COPY_TEXTURE_TO_TEXTURE + Dual Kawase blur + composite — the full pipeline, working today.
- **Backend-level (DisplayList-command-level) backdrop filter is the gap.** `Backend::render_backdrop_filter` at `backend.rs:805-834` is called when a `DrawCommand::BackdropFilter` is dispatched from within a layer's display list, not from a top-level `BackdropFilterLayer`. The current implementation has a `tracing::warn!` + child-passthrough fallback.

The two paths are distinct architectural levels:

```
Path A (working today):
  scene.render()
   → render_layer_recursive(BackdropFilterLayer)
   → handle_backdrop_filter()              // renderer.rs:845
   → flush + blur + composite

Path B (gap today):
  scene.render()
   → render_layer_recursive(PictureLayer)
   → picture.render() → dispatch_command(BackdropFilter, ...)
   → Backend::render_backdrop_filter()     // backend.rs:805  ← UNIMPL
```

Path B occurs when a backdrop filter is recorded as a `DrawCommand` inside a `DisplayList` (rather than as a standalone `BackdropFilterLayer` in the layer tree). This happens when, for example, a render object emits a backdrop blur via `Canvas::draw_image_filtered_backdrop()` or similar.

### Target shape

Path B re-uses the same offscreen + blur pipeline as Path A, factored into a private helper on `Backend`:

`crates/flui-engine/src/wgpu/backend.rs`:

```rust
impl Backend {
    /// Apply a backdrop filter to the surface contents within `bounds`,
    /// then dispatch the child display list on top.
    ///
    /// Flutter parity: `dart:ui::Canvas.saveLayer` with an `ImageFilter`
    /// is recorded as a `BackdropFilter` op in the display list; this
    /// is the engine-side dispatcher for that op.
    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        filter: &flui_painting::display_list::ImageFilter,
        bounds: Rect<Pixels>,
        _blend_mode: BlendMode,
        transform: &Matrix4,
    ) {
        use flui_painting::display_list::ImageFilter;

        // Path B uses the same Arc<Mutex<OffscreenRenderer>> the
        // Backend already owns. Without it, fall back to passthrough.
        let Some(offscreen_arc) = self.offscreen.clone() else {
            tracing::warn!("Backdrop filter: no OffscreenRenderer; dispatching child only");
            if let Some(child) = child {
                for command in child.commands() {
                    dispatch_command(command, self);
                }
            }
            return;
        };

        // Extract sigma; non-blur filters fall back to passthrough.
        // Mirrors Renderer::handle_backdrop_filter's filter-match (renderer.rs:861-880).
        let sigma = match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => f32::midpoint(*sigma_x, *sigma_y),
            other => {
                tracing::warn!(
                    "Backdrop filter type {:?} not yet supported in DisplayList path; child-only",
                    other
                );
                if let Some(child) = child {
                    for command in child.commands() {
                        dispatch_command(command, self);
                    }
                }
                return;
            }
        };

        // Stage 1: flush current painter batches so the surface holds the
        // backdrop pixels we want to blur. The DisplayList-command path
        // doesn't have a surface_view (Path A receives it from the
        // recursive layer traversal); the Backend must thread an
        // off-screen render-target through. ARCHITECTURAL NOTE: Path B
        // and Path A converge only if Backend has access to a destination
        // texture+view to blit into.
        //
        // The Backend type does not currently carry &surface_view. The
        // patterns are:
        //   (a) Stash the surface view+texture on the Backend at
        //       Backend::with_offscreen construction time. This is the
        //       chosen path — Renderer::render() already has both
        //       handles at the moment it builds the Backend.
        //   (b) Pass surface_view+texture as fn args to
        //       render_backdrop_filter; this propagates through the
        //       CommandRenderer trait which is undesirable.
        //
        // Path (a) implies the Backend grows two fields (surface_view,
        // surface_texture) populated by Renderer::render(). Both are
        // Arc-able (wgpu::TextureView, wgpu::Texture). See migration
        // step 1.

        let (device, queue, format) = {
            let off = offscreen_arc.lock();
            (Arc::clone(off.device()), Arc::clone(off.queue()), off.surface_format())
        };

        let x = bounds.left().0.max(0.0) as u32;
        let y = bounds.top().0.max(0.0) as u32;
        let w = bounds.width().0.max(1.0) as u32;
        let h = bounds.height().0.max(1.0) as u32;

        // Stage 1: flush painter to surface texture (caller-provided).
        let Some(surface_view) = self.surface_view.as_ref() else {
            tracing::warn!("Backdrop filter: no surface_view bound; child-only fallback");
            if let Some(child) = child {
                for command in child.commands() { dispatch_command(command, self); }
            }
            return;
        };
        let Some(surface_texture) = self.surface_texture.as_ref() else {
            tracing::warn!("Backdrop filter: no surface_texture bound; child-only fallback");
            if let Some(child) = child {
                for command in child.commands() { dispatch_command(command, self); }
            }
            return;
        };

        let mut flush_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("DisplayList Backdrop Flush"),
        });
        if let Err(e) = self.painter.render(surface_view, &mut flush_encoder) {
            tracing::error!("DisplayList backdrop flush failed: {}", e);
        }

        // Stage 2: COPY surface region → blur_input texture (same as Path A renderer.rs:907-925).
        let blur_input = { offscreen_arc.lock().texture_pool().acquire(w, h, format) };
        flush_encoder.copy_texture_to_texture(/* … standard wgpu copy, same as Path A … */);
        queue.submit(std::iter::once(flush_encoder.finish()));

        // Stage 3: Dual Kawase blur.
        let blurred = { offscreen_arc.lock().render_blur(&blur_input, sigma) };

        // Stage 4: queue blurred result for compositing on next painter flush.
        self.painter.queue_offscreen_result(blurred, bounds);

        // Stage 5: dispatch child display list on top of the blurred backdrop.
        if let Some(child) = child {
            self.with_transform(transform, |_painter| {});
            for command in child.commands() {
                dispatch_command(command, self);
            }
        }
    }
}
```

The new fields on `Backend`:

```rust
pub struct Backend {
    painter: WgpuPainter,
    offscreen: Option<Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>>,
    offscreen_painter: Option<WgpuPainter>,
    // NEW: surface handles for Path B backdrop-filter flushing
    surface_view: Option<Arc<wgpu::TextureView>>,
    surface_texture: Option<Arc<wgpu::Texture>>,
}
```

`Backend::with_offscreen` keeps its signature; a new `bind_surface(&mut self, view: Arc<wgpu::TextureView>, texture: Arc<wgpu::Texture>)` method is added and called by `Renderer::render` immediately after constructing the Backend.

`wgpu::TextureView` and `wgpu::Texture` are not `Arc`-able by default — wgpu types are owned. **Refinement:** instead of `Arc<wgpu::TextureView>`, the Backend stores `Option<&'frame wgpu::TextureView>` via a `'frame` lifetime; this couples Backend's lifetime to one frame. The `Backend` struct gains a lifetime parameter `Backend<'frame>`. Alternative: borrow each frame via a `bind_surface` / `unbind_surface` pair, with the unsafe `unsafe fn bind_surface(&mut self, view: *const wgpu::TextureView)` — rejected, no `unsafe` allowed in `flui-engine` outside `Renderer::new`'s already-audited block.

**The lifetime-parameter approach is the chosen path** per *Rust for Rustaceans* ch.2 "Variance and Lifetimes": Backend becomes `Backend<'frame>`, where `'frame` is the lifetime of the surface output. `Renderer::render` already holds `let view = output.texture.create_view(...)` for the duration of the frame — this is `'frame`.

```rust
pub struct Backend<'frame> {
    painter: WgpuPainter,
    offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>,
    offscreen_painter: Option<WgpuPainter>,
    surface_view: Option<&'frame wgpu::TextureView>,
    surface_texture: Option<&'frame wgpu::Texture>,
}

impl<'frame> Backend<'frame> {
    pub fn new(painter: WgpuPainter) -> Self { /* …  surface_view/texture: None …  */ }

    pub fn with_offscreen(
        painter: WgpuPainter,
        offscreen: Arc<parking_lot::Mutex<OffscreenRenderer>>,
    ) -> Self { /* …  surface_view/texture: None …  */ }

    /// Binds the frame's surface handles. Called by `Renderer::render`
    /// immediately after constructing the Backend, before any draw
    /// commands dispatch. Required for `render_backdrop_filter` to work
    /// via the DisplayList path.
    pub fn bind_surface(
        &mut self,
        view: &'frame wgpu::TextureView,
        texture: &'frame wgpu::Texture,
    ) {
        self.surface_view = Some(view);
        self.surface_texture = Some(texture);
    }
}
```

The `CommandRenderer` trait impl (`crates/flui-engine/src/traits.rs:199`) keeps `render_backdrop_filter`'s signature — the lifetime is internal to `Backend`. Backend's `CommandRenderer` impl moves to `impl<'frame> CommandRenderer for Backend<'frame>`.

### Migration order

**Commit 1 — Add `'frame` lifetime to Backend + bind_surface().** `Backend` becomes `Backend<'frame>`. `Backend::new` and `Backend::with_offscreen` populate `surface_view: None, surface_texture: None`. Add `bind_surface` method. Update `Renderer::render` at `renderer.rs:665-724` to call `backend.bind_surface(&view, &output.texture)` after construction. Update `Backend::into_painter` and `CommandRenderer` impl to carry the lifetime. **Compiles. Tests pass.** Subject: `feat(engine): U-7 add 'frame lifetime to Backend with bind_surface()`.

**Commit 2 — Implement `Backend::render_backdrop_filter` body.** Replace the stub at `backend.rs:805-834` with the full pipeline factored above. The body mirrors `Renderer::handle_backdrop_filter` (renderer.rs:845-960) using the bound surface handles. Add a private helper `Backend::backdrop_filter_inner` that returns `Result<(), &'static str>` for cleaner error paths (no `unwrap`). Update related tests at `backend.rs` test module. **Compiles. Tests pass.** Subject: `feat(engine): U-8 wire DisplayList backdrop filter through offscreen pipeline`.

### Question: is filter-rendering in scope for Wave 2 or splits to Wave 3?

**In scope.** The audit's E-2 fix shape names three sub-tasks (ownership rotation, painter API expose, actual filter rendering). But ownership rotation is not needed — Backend already owns the offscreen Arc. Painter API expose is not needed — the painter's `queue_offscreen_result` already exists and is used by Path A. The actual filter rendering is the only step, and it's a single PR-sized implementation matching the existing Path A code 95%. Splitting it would force a no-op intermediate commit that violates atomic-commit-per-unit.

### Risk + reversal

- **Risk:** `Backend<'frame>` lifetime parameter ripples to every callsite that names the type. The `CommandRenderer` trait is `&mut self` — its impls don't see the lifetime in the trait signature. Concrete callsites in `renderer.rs:669-712` and `backend.rs:425` (`let mut temp_backend = Backend::new(temp_painter);`) need lifetime annotation, but `Backend::new(painter)` with no bound surface infers `Backend<'_>` from context. The mask-rendering temp_backend at `backend.rs:425` never calls backdrop-filter; its `surface_view`/`surface_texture` stay None.
- **Risk:** Backend's existing `into_painter()` consumer doesn't know about the lifetime. With `Backend<'frame>`, `into_painter(self) -> WgpuPainter` works fine — the painter doesn't carry the lifetime. The borrow ends when Backend is consumed.
- **Reversal:** if the `'frame` lifetime parameter causes downstream churn, the alternative is to make Backend not parametric and pass surface_view + surface_texture as fn args to `render_backdrop_filter`. This requires expanding the `CommandRenderer` trait signature (and updating its `Debug` and `wgpu/debug.rs` stub impls), which is more invasive. The lifetime-parameter approach is preferred.
- **Verification gate:** `cargo build -p flui-engine --lib` clean; `cargo test -p flui-engine --lib` green; visual smoke-test (manual) of a backdrop-filter-via-DisplayList scene to confirm blur renders.

---

## Wave 2 commit-by-commit plan

Numbered for orchestrator reference. Each commit subject is a one-liner suitable for `git commit -m`; trailer is `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`.

| # | Unit | Subject | Touches (files) | LOC delta |
|---|------|---------|-----------------|-----------|
| 1 | R-6 | `refactor(rendering): U-1 reshape RendererBinding::render_views to four typed primitives` | `binding/mod.rs`, `flui-app/src/bindings/renderer_binding.rs` | +130 / −60 |
| 2 | R-7/8/9 step 1 | `feat(interaction): U-2 add HitTestResult paint_offset_scope / paint_transform_scope guards` | `flui-interaction/src/routing/hit_test.rs` | +50 / 0 |
| 3 | R-7/8/9 step 2 | `refactor(rendering): U-3 normalize BoxHitTestEntry signature to (RenderId, Matrix4)` | `flui-rendering/src/hit_testing/entry.rs` | +30 / −80 |
| 4 | R-7/8/9 step 3 | `refactor(rendering+app): U-4 collapse rendering HitTestResult to interaction canonical` | `flui-rendering/src/hit_testing/{result,mod}.rs`, `binding/mod.rs`, `view/render_view.rs`, `flui-app/src/{bindings/renderer_binding.rs,app/binding.rs}` | +250 / −880 |
| 5 | R-7/8/9 step 4 | `refactor(rendering): U-5 delete HitTestTarget trait + DummyTarget stubs (1-impl vestigial surface)` | `flui-rendering/src/hit_testing/{target,result,entry,mod}.rs`, `view/render_view.rs` | +0 / −180 |
| 6 | R-7/8/9 step 5 | `refactor(rendering+app): U-6 collapse rendering MouseTracker to interaction canonical` | delete `flui-rendering/src/input/`, update `binding/mod.rs`, `lib.rs`, `flui-app/src/bindings/renderer_binding.rs` | +60 / −720 |
| 7 | R-7/8/9 step 6 | `refactor(rendering): U-7 prelude + docs cleanup post-MouseTracker/HitTestResult absorption` | `flui-rendering/src/lib.rs`, doc strings | +20 / −60 |
| 8 | E-2 step 1 | `feat(engine): U-8 add 'frame lifetime to Backend with bind_surface()` | `flui-engine/src/wgpu/backend.rs`, `renderer.rs` | +60 / −20 |
| 9 | E-2 step 2 | `feat(engine): U-9 wire DisplayList backdrop filter through offscreen pipeline` | `flui-engine/src/wgpu/backend.rs` | +120 / −22 |
| 10 | receipts | `docs: U-10 cycle 4 Wave 2 receipts — verify all four P0 architectural reshapes` | `docs/research/2026-05-22-cycle4-wave2-receipts.md` (new) | +200 / 0 |

(Step #2 from R-7/8/9 is a small one because the rest of the interaction-side already absorbs; most of the trio's LOC delta lands in step #4 and #6 as deletions.)

If the Wave 2 PR feels too large at review time, the orchestrator's natural split is `commits 1 + 8 + 9 + 10` (R-6 + E-2 + receipts) into PR-A, and `commits 2-7` (trio) into PR-B. PR-B is by far the larger change.

---

## Migration receipts (verification gates table)

For each commit, the orchestrator requires the following gates to be **green** before the next commit lands. Pre-commit hook responsibility.

| Gate | Commit | Expected outcome |
|------|--------|------------------|
| `cargo build --workspace` | 1-10 | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 1-10 | clean |
| `cargo test -p flui-rendering --lib` | 1-10 | green |
| `cargo test -p flui-interaction --lib` | 2-10 | green |
| `cargo test -p flui-engine --lib` | 1, 8, 9, 10 | green |
| `cargo test -p flui-app --lib` | 1, 4, 5, 6, 10 | green |
| `bash scripts/port-check.sh -v` | 1, 4, 5, 6, 9, 10 | 7 refusal triggers clean |
| `rg 'flui_rendering::hit_testing::HitTestResult' crates/` | post-4 | zero hits |
| `rg 'HitTestTarget' crates/` | post-5 | zero hits |
| `rg 'flui_rendering::input' crates/` | post-6 | zero hits |
| `rg 'render_views\(\)\.' crates/` | post-1 | zero hits |
| `rg 'TODO: Convert' crates/flui-app/` | post-4 | zero hits (the literal bridge TODO at binding.rs:508-509 disappears) |
| `cargo doc --workspace --no-deps` | post-7, post-9 | clean (no broken rustdoc links) |

The U-10 receipts unit produces a `docs/research/2026-05-22-cycle4-wave2-receipts.md` that records each gate's actual outcome (commit hash, command, exit status) — same shape as cycle 3 PR #102's receipts (`docs/research/2026-05-22-cycle3-pr102-receipts.md` template).

---

## Risk register

| ID | Risk | Severity | Mitigation | Reversal |
|----|------|----------|------------|----------|
| RR-1 | The `flui_interaction::HitTestResult::new` initializes `transforms: vec![Matrix4::identity()]`; rendering-side protocol callers may depend on empty initial stack | Med | Audit `push_offset`/`push_transform` callsites in `context/hit_test.rs` and `protocol/box_protocol.rs` before commit 4 lands. Add regression test if needed. | Reshape interaction-side `HitTestResult::new` to start with empty `transforms` (Flutter parity check needed). |
| RR-2 | RenderView root-entry semantics drift (HitTestEntry without a real target ID) | Low | Introduce `RenderId::root_sentinel()` or use `RenderId::new(1)` consistently; verify gesture-handler dispatch correctly skips entries with sentinel target | Restore `new_render_view()` constructor with a fresh sentinel encoding |
| RR-3 | Multi-window MouseTracker callback drift (view_id parameter loss) | Low | Wrap the hit-test callback with view_id baked in at binding-impl construction time; defer multi-window full-shape to a future cycle when multi-window itself ships | None needed — single-window remains the default; multi-window has no consumers today |
| RR-4 | `Backend<'frame>` lifetime parameter ripples to test-only callsites that construct `Backend::new(painter)` without binding surface | Low | The lifetime is inferred at `Backend::new` callsites where surface is never bound (test code, offscreen-painter-for-shader-mask path); no source change at those sites | Roll back commit 8; pass surface_view+surface_texture as fn args via CommandRenderer trait (more invasive) |
| RR-5 | Two atomic commits (#3+#4, #6+#7) are bundled because the deletion-and-replacement is structurally inseparable | Low | The bundles each preserve atomic-compile property — `cargo build` is green at the bundle boundary, not between half-bundle steps. This matches PR #109's mechanical-deletion bundles. | None — splitting them produces non-compiling intermediate states |
| RR-6 | Total Wave 2 PR LOC delta (~−910 net) sits at the upper edge of single-PR review tolerance | Med | If reviewer feedback splits, the natural seam is "trio in one PR, R-6 + E-2 + receipts in another". Orchestrator pre-warns. | Run the split as Wave 2a + Wave 2b at PR-open time |
| RR-7 | `HitTestTarget` deletion may break downstream consumers I have not audited | Low | `rg 'HitTestTarget' crates/` returns 3 hits (1 production + 2 stubs). External `flui-*` crates do not import `flui_rendering::hit_testing::HitTestTarget` per workspace search. | Restore as a `#[deprecated]` shim in commit 5+1 if external consumer surfaces |

---

**Document status:** Design complete. Ready for orchestrator review and Wave 2 PR execution.
