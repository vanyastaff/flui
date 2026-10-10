# flui-objects Architecture

Per-crate ledger for the concrete `RenderBox` / `RenderSliver` catalog.
Decisions that span more than one module in this crate land
here. Module-level `//! # Mapping decisions` comments may repeat a local note
and should cite this file when the contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far.

---

## Module map

| Object | FLUI module | Notes |
|---|---|---|
| `RenderSliverFixedExtentList` | [`src/sliver/sliver_fixed_extent_list.rs`](src/sliver/sliver_fixed_extent_list.rs) | Index math + request-strategy retain band; see mapping decision below. |
| `RenderSliverGrid` | [`src/sliver/sliver_grid.rs`](src/sliver/sliver_grid.rs) | Delegate-windowed geometry + the same finite-domain scroll-window policy. |

---

## Mapping decisions

### Floating-header snap subscriptions follow the attached controller

Both floating-header modes retain their current invalidation handle while
attached. Replacing or withdrawing a snap controller removes the outgoing
value subscription and installs the replacement outside controller borrows.
Detach withdraws the invalidation handle before removing the current listener;
the widget continues to own cancellation and controller retirement.
`harness_swapping_the_snap_controller_moves_the_layout_listener` tests initial
absence, replacement, withdrawal and detach through complete render frames
for both floating and floating-pinned headers. The frame's layout telemetry
distinguishes callbacks from an outgoing controller from the new controller.

### Hidden layout retention does not imply accessibility retention

`RenderVisibility` lays its child out even when hidden. Its child participates
in semantics only when visible or explicitly retained with `maintain_semantics`.
This uses `RenderBox::visits_child_for_semantics`, independently of paint and
hit testing. A visibility change requests paint and, unless semantics is
retained, a semantics update. Changing retention while hidden requests only
semantics; changing it while visible changes no current presentation.

`harness_visibility_keeps_child_geometry_while_hidden` pins layout retention,
and `harness_visibility_reports_effective_semantics_changes` pins public update
impacts. The widgets' `retained_visibility_hides_child_semantics_by_default` and
`retained_visibility_updates_semantics_without_changing_layout` assert the
assembled accessibility output and mounted configuration changes.

### An animated translation keeps its transform layer

**Rule:** `RenderTransform` paints a pure translation as a plain child offset,
with no layer, so a static `Transform.translate` costs nothing. A translation
that changes every frame inverts the trade: without a layer each tick repaints
the moved subtree.

**Choice:** `RenderAnimatedTransform` reports every finite, invertible,
non-identity matrix — pure translations included — through
`paint_effects().transform`, so a tick that stays in that class is a
composited-layer update. At rest on the identity there is no layer; crossing
identity ↔ layered ↔ degenerate (a scale of 0, or a matrix that overflows)
marks paint. Paint, hit-testing and coordinate mapping read one cached sample,
never the animation. Measured with the `slide_transition_tick` perf scenario
(`crates/flui-widgets/tests/perf.rs`, a slide over a 30-row panel under a
repaint boundary): one tick rebuilt 64 elements and painted 66 nodes through
the previous rebuild-per-tick `SlideTransition`, and rebuilds 0 and paints 3
here.

**Test:** `harness_animated_transform_tick_dirty_marking`.

### Non-finite scroll-window edges never reach `f32 as usize`

**Rule:** Rust's `f32 as usize` saturates (`+∞ → usize::MAX`, `NaN → 0`), so
malformed scroll/cache constraints can silently mint a retain band or build window at
`usize::MAX` and freeze the viewport without a crisp error.

**Choice:** `RenderSliverFixedExtentList` and `RenderSliverGrid` share one
finite-domain policy for the cache-extended window:

| Edge | Value | Behavior |
|------|-------|----------|
| Leading (`scroll_offset + cache_origin`) | `NaN` or `+∞` | Reject: empty retain band `(0, 0)`, report the declared scroll extent, `tracing::error!` naming the render object and constraint fields. |
| Leading | `−∞` or negative finite | Clamp to `0` via `.max(0.0)` — same as a negative finite offset; layout proceeds from the origin. |
| Trailing (`leading + remaining_cache_extent`) | Finite | Normal index conversion, clamped to `item_count − 1`. |
| Trailing | `+∞` | Intentional unbounded window (shrink-wrap); existing `MAX_UNBOUNDED_WINDOW_CHILDREN` / `UNBOUNDED_SENTINEL_WINDOW` truncation. |
| Trailing | `NaN` or `−∞` | Reject: same empty-band fallback as a poisonous leading edge — not an unbounded window. |

Public index helpers on the fixed-extent list apply the same poison test to a
single offset (`NaN`/`+∞` → `0` + error; `−∞` → `0` without error).

**Alternatives considered:**

- Rely on `SliverConstraints::is_normalized()` / debug asserts alone — too late
  and not a production guard on the hot layout path.
- Panic / return a layout `Result` — the sliver protocol today commits geometry
  infallibly; a local empty window is the recoverable fallback.
- Clamp `+∞` leading to `0` like `−∞` — would hide a genuine overflow and lay
  out the wrong end of a large list.

**Trade-off:** malformed constraints that previously continued with a
saturated index now produce an empty window and a structured error log.
That is louder than silent saturation and quieter than a crash; diagnostics name the sliver type and the offending fields so production
reports identify the bad window.

**Tests:** unit tests on
`sliver_fixed_extent_list::window` / index helpers and on
`sliver_grid::classify_cache_window` / poison geometry prove the contract;
a healthy viewport cannot inject non-finite scroll edges into the harness.

---

### `RenderParagraph` publishes no semantics node for empty text

**Rule:** a text-less paragraph contributes no semantics (plain-text branch only,
the only one this object's V1 scope supports; see `src/text/paragraph.rs`'s
module doc "Out of scope").

**Choice:** `RenderParagraph::describe_semantics_configuration` sets neither
`label` nor `text_direction` when `TextPainter`'s plain text is empty, leaving
the configuration un-annotated.

**Why:** `SemanticsConfiguration::set_text_direction` (like
every other setter in `flui-semantics`) calls `mark_annotated()` on its own,
with no emptiness check — so setting them unconditionally would mark *every*
text-less paragraph in a tree as contributing semantics (`has_been_annotated
== true`), which forms or merges an empty, unlabelled node wherever a
`RenderParagraph` sits directly under a semantics boundary or an
explicit-child-node ancestor. The headless test harness constructs the empty
case (see `crates/flui-widgets/tests/semantics.rs`).

**Alternatives considered:**

- Set `label`/`text_direction` unconditionally — rejected: publishes a spurious node (or spurious merge
  input) for any empty-text paragraph, which is strictly worse for an
  assistive-technology consumer than publishing nothing.
- Fix `set_text_direction` to skip `mark_annotated()` for a "no-op" value —
  rejected: `text_direction` has no such value (`Ltr`/`Rtl` are both
  meaningful), and the fix would have to special-case this one call site
  rather than the setter, which is what this decision does instead, kept
  local to `RenderParagraph`.

**Trade-off:** an empty-text `RenderParagraph` publishes nothing at all, not even a (redundant) text direction. No known
consumer depends on an empty-labelled paragraph node's `text_direction`
alone; the alternative (a phantom node with no label) is the actively worse
default.

**Tests:** `crates/flui-widgets/tests/semantics.rs` mounts a
`Text` with empty content and asserts no node forms; a `Text("hello")` mount
asserts the label does.

---

### Subtree anchor rebinding preserves mounted identity

`RenderSubtreeAnchor::set_anchor` transfers its mounted `RenderId` to a new
`SubtreeAnchor`, clearing the old slot only if it still contains this node's ID.
Detach uses the same ownership check, so a sibling swapping slots or taking over
a slot cannot have its publication erased by the previous publisher. Before attach or after detach, rebinding
publishes nothing. Reusing the same slot is a no-op. This identity-only operation
does not invalidate geometry or replace the child. `AnchoredBox` calls it during
render-object updates, so reconciliation preserves child state when its anchor changes.
This extends FLUI's identity proxy contract. The `harness_subtree_anchor_attach_*` and `harness_subtree_anchor_detach_*` rows of
`family_layer_links` cover publication lifetime, and the widgets `anchored_box` regression rebuilds the
real element tree and checks preserved render IDs and child state.

---

### Text objects measure through the context their layout lends

`RenderParagraph` and `RenderEditable` acquire fallible `ctx.text()?` loans for every
`TextPainter` measurement: `perform_layout`, the four intrinsics,
`compute_dry_layout` and `compute_dry_baseline`. The context is the UI runtime's
`TextContext`, lent by the pipeline (flui-rendering's "Layout contexts lend the
UI runtime's text context"), so a paragraph measures with its own UI runtime's fonts
rather than an ambient collection (ADR-0092 §10 step 3; flui-painting mapping decision 14), and
paints the runs of the layout that measured (step 4). Every
`harness_*` test for both objects runs through the lent context. flui-runtime's
`two_ui_runtimes_measure_text_through_their_own_contexts` shows the loan reaches a
UI runtime's context through a mounted `Text`.

---

### Finite sizing survives intermediate normalization overflow

An intrinsic quantum smaller than the representable spacing of a finite size
cannot enlarge that size. `RenderIntrinsicWidth` keeps the finite intrinsic
answer when dividing by its positive quantum overflows, while ordinary stepping
still rounds upward. `family_intrinsics` checks a tiny quantum against the
child's natural width and an ordinary step against the next multiple.

`family_sizing` also checks a large finite constrained width against a distinct
parent maximum, plus ordinary hundredth quantization. Rounding extra constraints
must not turn that finite width into infinity and force the parent maximum.

### Aspect-ratio sizing admits finite geometry deliberately

With normalized finite nonnegative minima and finite or unbounded maxima,
`RenderAspectRatio` selects a finite size covering the minima where the ratio
permits it. Intermediate overflow is capped at the representable range; parent
constraints take precedence when preserving the ratio is impossible. Invalid
unchecked factors use the minimum size. Infinite minima cannot admit finite
geometry and reach the pipeline's typed geometry rejection, without a clamp
panic. The `family_sizing` rows
`aspect_ratio_unbounded_minima_remain_finite` and
`aspect_ratio_infinite_minima_reject_without_panicking` exercise the real
constrained-parent producer and recovery after a finite parent update.

### Editable text scrolls its content within the allocated viewport

`RenderEditable` keeps full text layout for single-line caret movement while
clipping text, selection, composition and caret to the allocated viewport.
Painting subtracts the horizontal displacement; pointer lookup adds it back.
Caret movement reveals the active caret without changing the allocated width.
An absent caret-height override resolves to the shaped single-line height.
Paint and collapsed-range/IME rectangles use that same resolved height;
dry layout and intrinsics derive it from their own text measurement. Explicit
heights remain logical lengths. The mounted widget contract
`inherited_text_sizing_updates_editable_glyphs_and_caret` covers scaled and
empty text, decorated fields and override removal.
The widgets text-editing contract table checks actual editing and pointer
selection through the viewport, including RTL and obscured text.

## Thread safety

No locks. Catalog objects are mutated on the UI runtime's layout/paint thread
through `PipelineOwner`; they hold no shared mutable state of their own.

---

## Friction log

| Item | Notes |
|------|-------|
| Shared `classify_cache_window` helper across list + grid | Grid owns its classify today; list uses `finite_leading_cache_edge`. Consolidating into one module is optional follow-up once a third consumer appears. |


### RenderImage source fitting

RenderImage uses painting's BoxFit::apply for both the cropped source and the
fitted destination (ADR-0115). Alignment positions both rectangles. Its logical
intrinsic/scale source converts back to decoded texel coordinates before Canvas
records an ImageRegion, so Cover paints inside the allocated box and high-DPI
source cropping uses the decoded image's coordinates. The actual object/tree
consumer is `render_image_scaled_cover`, a row of the engine's existing
`painter_images_and_offscreen_results_read_back_as_specified` GPU family; the
objects catalog still owns its layout/paint-presence harness rows.
