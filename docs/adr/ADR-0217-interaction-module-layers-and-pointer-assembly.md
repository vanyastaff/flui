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
   | 0 | `events`, `ids`, `retain` |
   | 1 | `close`, `containment` (they name `retain`, and modules in one layer name none of each other) |
   | 2 | `pointer`: hit test, interaction lane, router, capture, mouse tracker, `resampler` and `sampling_clock` (out of `processing`), `wheel_preferences`, `InputSequenceId` minting; it consumes the input `flui-runtime` routes from `HostInput` (ADR-0207) and owns no boundary of its own |
   | 2 | `focus` (`focus`, `focus_scope`, `traversal`, moved out of `routing`); `text_input` (`text_input`, `clipboard`) |
   | 3 | `gesture`: `arena`, `recognizers`, `velocity`, `lsq_solver`, `one_euro` (out of `processing`; `Velocity` by one path), `settings`, `details`, `traits`, `observability`, DnD sessions |
   | 4 | `binding` |
   | 5 | `__runtime` (the runtime's terminal-cleanup seam) and `testing` (feature; test builders that drive the binding, so it holds code and is not transparent) |
   | — | `prelude` is transparent (only `use` items) |

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
   Assembly allocates nothing per event. A contact that can hover (mouse, pen) survives its
   `Up`: it returns to hovering on the same pointer identity, a later `Move` hovers on that
   identity, and the platform's leave (`WM_POINTERLEAVE`, the end of a UIKit hover, out of
   range) ends a pen, so its `Enter` and `Leave` pair up; a mouse stays for the life of its
   device. A touch has no hover and ends at `Up`. A hovering pen whose label comes back as
   another kind left without a leave: it gets one, then the reading starts a new contact (row
   `a_lifted_pen_hovers_on_its_identity_until_it_leaves`).
4. **Two-finger pan is `Scroll`.** On every OS a two-finger trackpad pan is `Scroll` with
   `ScrollPhase`, and platform momentum is `ScrollPhase::Momentum*`. `PanZoom` carries only scale
   and rotation, about a defined focal point. A session's consumer is chosen along the path
   hit at its `Start` focal point and kept like a captured pointer however far the focal
   point moves; raw observers follow fresh geometry (row
   `viewer_keeps_a_pinch_whose_focal_point_leaves_it`). This amends ADR-0143: `PanZoomTransform` loses its
   pan component, and translation during a pinch is carried by the focal point, updated on every
   `Update`. On Windows DirectManipulation serves only the
   precision touchpad; touch stays raw contacts.
5. **`InputRegions`.** The protocol lives in `flui-rendering`, the render object in
   `flui-objects` (a `RENDER_OBJECT_TYPES` entry with harness rows), and the
   `SystemGestureExclusion` widget in `flui-widgets`. Render objects write `Rect`s in their own
   local coordinates, tagged by a `#[non_exhaustive]` `InputRegionKind`, into the frame's
   `&mut InputRegionSink`. The sink maps each to window-logical coordinates with the paint
   transform (the axis-aligned bounding box under a rotation or skew), intersects it with the
   effective clip and drops an empty result. A region whose transformed or projected corners
   are not finite (overflow in the transform or in the device scale) is dropped with a
   diagnostic, never clamped from infinity. Under a perspective transform the homogeneous `w`
   of all four corners must be positive (above a small epsilon); since `w` is affine over the
   rectangle, that guarantees no point of it reaches or crosses the `w = 0` horizon, and a
   region failing it is dropped with a diagnostic even when the corner quotients are finite
   (row `a_region_crossing_the_perspective_horizon_is_dropped`); a finite region beyond the window is clamped to the
   window bounds; the conversion to device pixels rounds outward with the existing
   `flui_foundation::device_rect_covering` (floor of minima, ceiling of maxima), so a thin
   region at fractional DPI is never erased; row `fractional_region_edges_round_outward`. Before
   that conversion the backend checks that the rounded device edges, and the width and height
   they span (`max - min`), are representable as `i32`; a region that is not is dropped with a
   diagnostic, never saturated by the cast or wrapped (rows
   `device_edges_just_inside_the_i32_range_are_kept`,
   `device_edges_outside_the_i32_range_drop_the_region` and
   `edges_inside_the_range_whose_span_exceeds_i32_drop_the_region`). The
   framework hands `PlatformWindow::set_input_regions` window-logical rectangles; the backend
   performs the single projection (with that outward rounding) to device pixels using the
   window's current scale, so no layer scales twice. Regions are collected per window at
   compositing.
   The backend's diff key is the projected device-pixel set plus the scale and the window's display as `Option<DisplayId>` (`None` where `PlatformWindow::display()` reports none; a change between `None` and `Some` is a key change),
   so a scale or display change (including between displays of equal scale) re-sends an
   otherwise equal logical set. The diff lives entirely in the backend: the framework calls
   `PlatformWindow::set_input_regions` with the window's logical regions on every composited
   frame (a handful of rectangles), and the backend projects them, compares the key with the
   previous frame's and updates the OS only when it differs. No caller-side comparison can
   therefore hide a display or scale change from the backend. The first
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
- `harness_input_regions` in `render_object_harness`, and in `flui-widgets`
  `input_regions_reach_the_os_only_when_changed`, `a_transformed_region_reports_its_bounding_box`,
  `a_clipped_region_reports_the_intersection`, and
  `a_scale_change_resends_unchanged_logical_regions`, `a_display_change_at_equal_scale_resends_regions`, `a_degenerate_transform_drops_the_region_with_a_diagnostic`.

The decoders run in Linux CI over recorded readings. Native capture, DirectManipulation and the
OS effect of exclusion regions run on a native host or device only.

## Alternatives rejected

- **`ui-events` as the assembled vocabulary.** Its `PointerGesture` has pinch and rotate without
  phases, and no sequence id, `Revise` or virtual kind (dependency research §7).
- **An R-tree (`rstar`) for regions.** Built for thousands of entries; here a handful.
- **A `PointerAssembler` trait per backend.** One implementation in practice; a trait would add
  dispatch without a second user.
