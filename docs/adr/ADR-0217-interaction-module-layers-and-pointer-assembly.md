# ADR-0217: `flui-interaction` is layered into modules, pointer assembly is shared by every backend, and input regions are collected from layout

- **Status:** Accepted (2026-10-13); implementation pending
  (`docs/plans/specs/flui-interaction/completeness/tasks.md`). Until it merges, `main` follows
  ADR-0143 and the current code: unlayered `flui-interaction` modules, per-backend pointer
  assembly, and a `PanZoomTransform` that carries pan.
- **Date:** 2026-10-13
- **Amends:** [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md): `PanZoomTransform`
  loses its pan component; a two-finger pan is `Scroll` with phases, and translation during a
  pinch is carried by the focal point, updated on every `Update`.
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (layers between crates;
  this record layers inside one), [ADR-0082](ADR-0082-platform-api-contract-crate.md) (platform
  types stay inside `flui-platform`), [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md)
  (the `Scroll` and `PanZoom` vocabulary), [ADR-0098](ADR-0098-owned-f64-geometry-values.md)
  (logical geometry of input regions), [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md);
  spec [docs/plans/specs/flui-interaction/completeness/](../plans/specs/flui-interaction/completeness/design.md);
  research [best practices](../research/2026-10-12-interaction-input-best-practices.md),
  [dependency selection](../research/2026-10-13-interaction-dependency-selection.md).

## Context

`flui-interaction` has no module order: recognizers import `routing::PointerDispatch`,
`__runtime` sits on an import cycle, and `Velocity` is public under two paths. `GestureBinding`
holds every piece of pointer, scroll, pan-zoom, hover and close state directly, so a service
cannot be tested or keyed per surface on its own. Each backend assembles pointers itself —
contact ids, device presence, held buttons, click counts, trackpad accumulation, move batching —
so the same rule is written up to six times and only the headless copy runs in CI. Windows
reports a two-finger trackpad pan as a pan-zoom transform and macOS as phased scrolling, so
consumers branch per OS. Nothing tells the OS which screen areas the app needs for its own
gestures (Android `setSystemGestureExclusionRects`, iOS deferred screen edges).

## Decision

1. **Module layers.** `flui-interaction` declares `[package.metadata.flui.modules]` with
   `exceptions = []`, checked by `cargo xtask module-dag`:

   | Layer | Modules |
   |---|---|
   | 0 | `events`, `ids`, `close`, `containment`, `retain` |
   | 1 | `pointer`: hit test, interaction lane, router, capture, mouse tracker, `resampler` and `sampling_clock` (out of `processing`), `wheel_preferences`, `InputSequenceId` minting; it consumes the input `flui-runtime` routes from `HostInput` (ADR-0207) and owns no boundary of its own |
   | 1 | `focus` (`focus`, `focus_scope`, `traversal`, moved out of `routing`); `text_input` (`text_input`, `clipboard`) |
   | 2 | `gesture`: `arena`, `recognizers`, `velocity`, `lsq_solver`, `one_euro` (out of `processing`; `Velocity` by one path), `settings`, `details`, `traits`, `observability`, DnD sessions |
   | 3 | `binding` |
   | — | `testing` (feature) and `prelude` are transparent |

   Modules move in mechanical commits; a moved `static` or `thread_local!` keeps its `globals`
   entry in the same commit. Each public type is reachable by one path.
2. **`GestureBinding` is a thin composition root.** Its services — `PointerSequences`,
   `MoveCoalescer`, `ScrollSequences`, `PanZoomSequences`, `MouseTracker`, `ArenaDriver`,
   `CloseCoordinator` — are `pub(crate)`, hold no reference back to the binding or each other,
   and are keyed by `InputSurface` from their first version. Closing sets a flag; on leaving
   dispatch each service yields its retired state through `drain(&self)` (`RefCell::take`), and
   user code runs only after every borrow is released.
3. **Shared pointer assembly.** `crates/flui-platform/src/shared/` gains a `pointer_assembly`
   module beside `events.rs`: `ContactRegistry`, `DeviceRegistry`, `HeldButtons`,
   `ClickCounter` (thresholds from OS settings), `PanZoomAccumulator`, `MoveBatch`. They are
   plain structs driven by pure functions over plain readings:

   ```rust
   pub fn decode(contacts: &mut ContactRegistry, reading: &NativeReading)
       -> Result<SmallVec<[PlatformInput; 2]>, DecodeError>;
   ```

   `NativeReading` holds no `windows::*` or `objc2` type; clock units are newtypes
   (`QpcTicks(u64)`, `NativeTime`). There is no trait with a single implementation. A reading
   that fails to decode (`DecodeError`) cancels its open contact with
   `CancelReason::InvalidInput` and a diagnostic; a reading that belongs to no open contact is
   dropped with a diagnostic. Nothing is half-applied. The
   registry keeps the originating native event per contact, so a native DnD can start from it.
   Assembly allocates nothing per event.
4. **Two-finger pan is `Scroll`.** On every OS a two-finger trackpad pan is `Scroll` with
   `ScrollPhase`, and platform momentum is `ScrollPhase::Momentum*`. `PanZoom` carries only scale
   and rotation, about a defined focal point. This amends ADR-0143: `PanZoomTransform` loses its
   pan component, and translation during a pinch is carried by the focal point, updated on every
   `Update`. On Windows DirectManipulation serves only the
   precision touchpad; touch stays raw contacts.
5. **`InputRegions`.** The protocol lives in `flui-rendering`, the render object in
   `flui-objects` (a `RENDER_OBJECT_TYPES` entry with harness rows), and the
   `SystemGestureExclusion` widget in `flui-widgets`. Render objects write logical `Rect`s,
   tagged by a `#[non_exhaustive]` `InputRegionKind`, into the frame's `&mut InputRegionSink`;
   regions are collected per window at compositing and passed to
   `PlatformWindow::set_input_regions` only when they differ from the previous frame. The first
   kind is system-gesture exclusion; pointer and title-bar regions follow. Storage is a per-window
   `Vec` scanned linearly: the OS consumer takes single digits of rectangles.

## Consequences and verification

A recognizer change cannot reach `binding`, a service is testable alone, every backend shares one
assembly path that Linux CI executes, and gesture consumers see one trackpad grammar.

- `cargo xtask module-dag` with no exceptions, in `checks`.
- `binding_services_are_keyed_by_surface` and `a_closed_binding_drains_services_after_dispatch`
  through the public binding.
- `pointer_assembly` rows in `flui-platform`'s tests: contact reuse after Up, device hot-plug,
  click count across the OS double-click interval, pan-zoom accumulation, move batching, a
  reading that fails to decode; plus a counting-allocator row for the per-event budget.
- `two_finger_pan_is_phased_scroll_on_every_backend` and
  `pan_zoom_carries_only_scale_and_rotation`, and
  `an_undecodable_reading_cancels_its_contact_as_invalid_input` (an open contact ends with one
  `Cancel`; an orphan reading emits nothing), as `assert_backend_conforms` rows
  ([ADR-0215](ADR-0215-input-recording-and-backend-conformance.md)).
- `harness_input_regions` in `render_object_harness`, and
  `input_regions_reach_the_window_only_when_changed` in `flui-widgets`.

The decoders run in Linux CI over recorded readings. Native capture, DirectManipulation and the
OS effect of exclusion regions run on a native host or device only.

## Alternatives rejected

- **`ui-events` as the assembled vocabulary.** Its `PointerGesture` has pinch and rotate without
  phases, and no sequence id, `Revise` or virtual kind (dependency research §7).
- **An R-tree (`rstar`) for regions.** Built for thousands of entries; here a handful.
- **A `PointerAssembler` trait per backend.** One implementation in practice; a trait would add
  dispatch without a second user.
