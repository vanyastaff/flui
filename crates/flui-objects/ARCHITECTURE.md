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

### Cross-axis sliver tracks preserve the scrolling protocol

Content and positively weighted flexible tracks use a dedicated parent-data
type; committed offsets stay in the render tree. Limits refuse nonfinite and
negative values before mutation. Layout stages physical positions, propagates
scroll corrections and withholds dimensions when those positions cannot be
represented. [ADR-0228](../../docs/adr/ADR-0228-cross-axis-sliver-tracks.md)
records allocation, composition and recovery. Its public render and mounted
widget rows cover both axes, reversed placement, weight updates and accessible
child bounds.

### Sliver annotations keep their original layout protocol

`RenderSliverSemanticsAnnotations` forwards its child's committed sliver geometry
and hit coordinates. It contributes the same `SemanticsConfiguration` as box
annotations and can exclude descendant accessibility independently of paint or
layout. The public consumer is `SliverSemantics`; no Box adapter or extra
viewport is introduced. Harness row
`harness_sliver_semantics_annotations_preserves_geometry_paint_and_hits` and
widget row `sliver_annotations_update_and_exclude_without_adapting_geometry`
check geometry, publication, exclusion and retained identity.

### Spoken span labels preserve rendered text and child content

`RenderParagraph` uses each span's semantics label in place of that span's own
spoken text, retaining its children in declarative order. An empty override
silences only the owning span. Label-only replacement invalidates semantics even
when the painter keeps its existing glyph layout. The mounted widget row
`rich_text_accessible_labels_follow_span_overrides_and_replacement` checks
published labels, nested overrides, empty labels, replacement identity and
unchanged text geometry.

### Stack intrinsic sizing uses only non-positioned content

Positioned overlays do not contribute to Stack's allocated size, so they do not
contribute to its intrinsic dimensions either. Querying their size would let an
IntrinsicWidth/IntrinsicHeight wrapper enlarge the container from overlay content
that ordinary layout excludes. The existing child parent-data query identifies
these children without a second positioning cache. IndexedStack installs ordinary
BoxParentData and continues to size from all children.
`harness_stack_intrinsic_sizing_ignores_positioned_overlays` observes real intrinsic
wrapper layout plus all four queries for edge-positioned, explicitly sized and
ordinary children at finite and unbounded cross-axis inputs.

### Fill-viewport geometry does not own a separate reveal policy

FillViewport eagerly lays out its attached children at viewport-fraction extents.
Its former implicit-scrolling flag had no behavioral consumer and is removed.
Registered semantics ancestry controls descendant reveal under ADR-0168/ADR-0225,
including materialized children beyond a cache window. The widget cases
`show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse` and
`show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry` observe
that contract through real scroll owners.

### Animation and viewport subscriptions end with their render owner

Animation samplers, opacity, transforms, animated size, placement tracks and
floating headers own cancellation independently of the widget's animation run.
Detaching or dropping a render object removes its listener without stopping or
disposing that run. Callback-capable owned registrations preserve each sampler's
generation checks and pending delivery; they do not replace sampling with a
plain invalidation callback. Opacity must cancel even when its own callback holds
the proxy strongly. `harness_animated_opacity_owner_release_releases_its_animation_parent`
observes both box and sliver parent lifetime through a mounted public tree.
The private sampler test
`owner_release_retires_samples_even_while_the_proxy_survives` observes cached-value
retirement with an independently retained proxy, a seam unavailable through the
concrete catalog's closed value types.

Viewport offsets cancel by callback identity rather than `ListenerId`. Their
private registration stores a cancellation function selected for the offset type;
owner destruction can therefore cancel without adding a public generic bound or
another shared owner of the offset. Callback activity is revoked before invoking
user cancellation. A replacement commits its offset first, then independently
cancels, subscribes and retires outgoing ownership through `PanicRecovery`.
Since callback identity is available before registration, even an unacknowledged
registration can attempt cancellation. Failed opaque retirement follows ADR-0127.
`harness_viewport_owner_release_cancels_offset_callbacks` and
`harness_viewport_replacement_finishes_subscription_migration_after_failure`
cover both viewport types, competing failures, stale callbacks and recovery.

### Delegate subscriptions own cancellation and revoke callbacks before callouts

CustomPaint and Flow retain the actual listenable and registration ID. Cleanup
does not call the delegate's source getter again. Each registration revokes its
relay before invoking cancellation, so a source that panics before removing the
callback cannot wake the current presentation through an outgoing registration.
RAII also cancels registrations on owner release. CustomPaint moves both
subscriptions and both painter owners out before exceptional destruction, so
an earlier cancellation or capture failure cannot strand the healthy tail.

Comparisons run before replacement admission. A failed comparison preserves the
old presentation and retires rejected ownership through `PanicRecovery`. Once
replacement commits, cancellation, new registration and outgoing retirement
are separate contained operations. If one fails, the committed update is
admitted through the invalidation handle before the first failure resumes:
an unwinding setter cannot return its impact to the normal caller.

Healthy ownership is destroyed normally. Failed opaque ownership follows
[ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md). A source that
registers then panics without returning an ID supplies no cancellation
capability; its relay is revoked and ownership retained. An upstream
cancellation that panics likewise cannot prove removal, although the relay is
already inert. These boundaries cannot rescue a user-owned aggregate that
double-panics internally before returning to the framework.

Public harness rows
`harness_custom_paint_detach_finishes_both_subscriptions_after_failure`,
`harness_custom_paint_owner_release_removes_external_subscriptions`,
`harness_custom_paint_failed_replacement_commits_new_painter_and_revokes_old_relay`,
`harness_custom_paint_comparison_failure_keeps_the_old_presentation`,
`harness_custom_paint_failed_subscription_is_inert_and_can_be_replaced`,
`harness_custom_paint_owner_release_preserves_first_retirement_failure`,
`harness_custom_paint_old_painter_retirement_cannot_erase_the_replacement` and
`harness_flow_subscription_migration_and_owner_release_finish_after_failure`
pin initial, single-failure, competing-failure and subsequent-operation paths.

### Flow accessibility follows presentation without owning another geometry cache

Flow's pure replay selects presented children and supplies their transforms.
`FlowPaintingContext::paint_child_with_opacity` records continuous group opacity
through the ordinary fragment recorder. Its zero-opacity call occupies the slot
but omits presentation, input and accessibility; partial opacity keeps child
geometry and actions. `harness_flow_child_opacity_reaches_compositing_and_input`
checks inline and repaint-boundary children. The mounted widget row
`flow_repaint_and_delegate_replacement_update_accessible_children` also pins
notifier-driven transparent/partial/opaque transitions.
The pipeline passes committed size to the semantics child-selection hook.
Its clip bounds accessible geometry, and repaint notifications admit both paint
and semantics work together. Logical reading order stays declarative even when
the delegate reverses paint order. [ADR-0224](../../docs/adr/ADR-0224-geometry-aware-semantics-child-selection.md)
records the protocol and alternatives. The public rows
`harness_flow_accessible_bounds_follow_clip_without_layout` and
`harness_flow_only_announces_presented_children_after_repaint_and_replacement`
pin bounds, visibility, actions, replacement and reading order.

### Flex resolves both physical axes before positioning

Text direction controls the horizontal axis, and `VerticalDirection` controls
the vertical axis, independently of which is the main axis. Reversing direction
preserves declarative child identity and baseline selection. Live and dry
baseline use the same positioned-child calculation.
`harness_flex_vertical_direction_positions_and_reports_baselines` covers both
axes and RTL, while `vertical_direction_updates_mounted_flex_children` covers
the widget configuration path and updates without remounting.

### Custom-painted accessibility follows paint traversal order

`RenderCustomPaint` supplies background regions, real child semantics, then
foreground regions. The two painter lists have independent key namespaces;
their regions use the normal ancestor merge, exclusion, blocking and geometry
rules rather than a single host annotation. [ADR-0223](../../docs/adr/ADR-0223-authored-semantics-regions.md)
records identity ownership and failure behavior.
Painter repaint notifications refresh regions as well as recorded paint;
`harness_custom_paint_repaint_refreshes_region_geometry_identity_and_actions`
checks both painter roles, retained identities, current actions and removal.
`harness_custom_paint_semantic_regions_keep_order_geometry_identity_and_actions`
and `harness_custom_paint_regions_obey_merge_exclude_and_action_blocking`
observe the actual assembly and action-resolution paths.

### Baseline selection follows the object's layout policy

Sizing and positioning wrappers forward through the pipeline's committed
child edges. Stack selects the highest natural line across both aligned and
positioned children using the same child-offset helper for live and dry
queries; `harness_stack_baselines_include_positioned_and_aligned_children`
checks both paths and subsequent loss of every natural line.
Wrap selects the highest positioned child baseline, sharing
`visit_offsets` between committed positioning and dry-baseline queries;
Theater considers only onstage children and clears its recorded lines when
none remain. LayoutBuilder's live forwarding does not enable speculative dry
layout. [ADR-0234](../../docs/adr/ADR-0234-layout-baseline-composition.md) records
the shared contract and FLUI's explicit authored-line policy for Baseline.
`harness_wrap_reports_its_highest_positioned_baseline`,
`harness_theater_baselines_follow_only_onstage_children` and
`harness_offstage_and_layout_builder_forward_live_baselines` observe these
rules through a real baseline-positioning parent.

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

Static box and sliver opacity omit child semantics at exactly zero opacity,
including when a layer is forced. Nonzero opacity retains accessibility even
when raster alpha rounds toward zero. Box opacity retains its independent
pointer hit-testing contract. `RenderSliverOffstage` excludes the whole
hidden subtree while retaining child layout. The assembled-tree and stale
action contracts are pinned by
`harness_static_opacity_visibility_controls_semantics_and_actions` and
`harness_sliver_offstage_removes_hidden_semantics_and_stale_actions`.

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

### Physical surfaces share their clip with accessible bounds

Physical surfaces derive the conservative accessibility clip from the same
local shape used to clip the child. Shadow elevation does not expand that
clip. `Clip::None` imposes no accessible clip; changing clip behavior updates
semantics without requiring layout. Bounds are conservative for curved and
nonrectangular paths, as with ordinary path clips.
`harness_physical_shape_clips_accessible_bounds_and_updates_without_layout`
checks initial clipping and transitions through the mounted pipeline.

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

The sample cache stays on the UI owner (ADR-0175): `Rc` shares whole-value
`Cell` records, without atomic buffers or reader/writer spin loops. A ticket
precedes each user read; an outgoing reading or delivery cannot overwrite a
newer record after reentry. Published and delivered records remain separate,
so a failed invalidation retries even after the value returns to its last
delivered value. Exhaustion preserves the last sample permanently. The
private recovery cases `failed_mark_retries`,
`newer_generation_wins_in_either_commit_order` and
`restoring_a_delivered_value_while_a_newer_one_is_in_flight_still_marks` pin
these obligations. A hit visit retains its selected sample independently.

The facade's `a_slide_tick_moves_pixels_without_rebuilding` also drives a
mounted `SlideTransition` through identity-to-layer and retained-layer ticks.
GPU interior samples observe the rectangle moving from zero to twenty and
forty pixels while the frame reports zero element builds. Suppressing delivery
after the first moving sample leaves a red pixel at the outgoing location,
distinguishing a stale retained transform from successful rasterization.

### Layout and decoration transitions relayout or repaint one node per tick

**Rule:** an explicit transition whose value changes layout (alignment, a size
factor, a stack rectangle) or a decoration would otherwise rebuild its child
subtree each tick, as a rebuild-per-tick builder does.

**Choice:** `RenderAnimatedAlign`, `RenderAnimatedInset` and
`RenderAnimatedDecoratedBox` each own their proxy and a cached sample
(`src/animation_sample.rs`): the listener reads the animation, publishes a
changed, admitted value and marks layout (align, inset) or paint (decoration)
on that node only. Frame walks read the cache; a non-finite layout sample is
refused and the last one stays; the listener holds the proxy weakly.
`RenderAnimatedAlign` delegates to a `RenderAlign` carrying the resolved
configuration rather than duplicating its sizing rules. `RenderAnimatedInset`
admits negative insets, which `RenderPadding` rejects, because a positioned
stack child may sit partly outside its stack. Filling its stack slot exactly,
it hides that overflow from the stack's clip, so it clips the child to its own
bounds itself (`harness_animated_inset_clips_a_child_outside_its_box`).

**Tests:** `harness_animated_align_tick_moves_the_child`,
`harness_animated_inset_tick_places_the_child_at_the_rect`,
`harness_animated_decorated_box_tick_repaints_without_layout`,
`harness_animated_align_refuses_a_non_finite_sample`; the widgets' rows of
`transitions_tick_without_rebuilding` assert zero element builds per tick.

### Aligning boxes bound their child offset, not the whole subtree's geometry

**Rule:** a finite but extreme alignment over a large free space overflows
`alignment × free space`, and offsets add along the paint, semantics and
hit-test walks.

**Choice:** `AligningShiftedBox::dry_child_offset` (shared by every aligning
box, live and dry) limits each offset component to `f64::MAX / 2^20` in the
negative direction and, in the forward direction, to `(f64::MAX − child
extent) / 2^20` capped at the same bound, and the dry baseline saturates at
`±f64::MAX`.

**Admitted range:** the immediate child's far edge (`offset + extent`) is
finite, and so is any chain of up to about 2^20 nested aligning boxes whose
offsets share a sign. Outside the range, and not handled: a descendant that
paints or reports bounds past its parent's reported extent, a composition of
extreme offsets of mixed sign through children that themselves overflow, and
nesting deeper than about 2^20 levels. Such geometry may reach the paint and
semantics walks as non-finite; making those walks saturate their accumulated
translation is a separate, pipeline-wide change.

**Tests:** `harness_nested_extreme_alignments_keep_paint_and_semantics_finite`,
`harness_oversized_child_keeps_paint_and_semantics_finite`,
`harness_oversized_child_keeps_centre_and_right_alignment`,
`harness_aligning_boxes_saturate_an_overflowing_dry_baseline`.

### Skeletons paint their own share of one shimmer band

**Rule:** every placeholder under one shimmer must show the same moving band,
and each placeholder is a separate node painting in its own coordinates.

**Choice:** `RenderSkeleton` lays a `ShimmerBand` gradient across its nearest
paint anchor (`RenderPaintAnchor`, also a repaint boundary) and translates it
by its own offset from the anchor (`PaintCx::paint_anchor`), so placeholders
cut windows out of one band without an offscreen group around them. The band
starts one band-width before the anchor's leading edge and ends past its
trailing edge, so a repeating phase has no seam. Without a band or without an
anchor the skeleton paints its flat base colour. `RenderShimmerMask` uses the
same band across a single child, through a `SrcATop` shader mask that tints
only what the child painted; it pushes the mask only mid-sweep.

**Tests:** `harness_skeleton_band_spans_its_paint_anchor`,
`harness_skeleton_paints_flat_without_anchor_or_band`,
`harness_skeleton_tick_repaints_without_layout`,
`harness_shimmer_mask_masks_only_mid_sweep`.

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

### Lazy slivers slide a moved child only across a roster change

`RenderSliverList` and `RenderSliverGrid` take an optional `Placement`: one
`PlacementSlot` per logical index, a clock the owning state binds and disposes
(ADR-0013: the render object only restarts it and listens), its motion, and the
owner's roster generation. After positioning, each pass compares every child's
content position (main-axis layout offset, cross-axis offset) with the one its
slot recorded in the previous pass, and on a move rebases every visible child's
remaining displacement onto a restarted run. Content positions make the scroll
offset cancel out; the displaced position goes through the sliver's own
placement function, so growth and cross-axis directions stay consistent, and
paint and hit testing read the same offset.

Two rules keep a slide to real reorders. A child not laid out in the previous
pass has no position to come from and appears in place, however long ago its
slot was last laid out. Comparison is armed only by a new generation, and stays
armed until a pass requests no child and corrects no scroll offset; a resize or
an estimate correction under an unchanged generation places children at once.
Displacements of the passes in between add up, so a child built mid-frame still
shifts its followers once. `harness_sliver_list_placement_slides_moved_children_into_place`
and `harness_sliver_grid_placement_slides_tiles_between_cells` pin both axes, the
unchanged-placement no-op and the unarmed resize; the widgets row
`an_item_entering_the_band_takes_its_place_without_sliding` pins the band rule
through a mounted list.

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

### A content shape is the hit area, not a filter over the child

`RenderContentShape` takes the hit anywhere inside its `HitShape`, even where
its child paints nothing, and rejects everything outside it without testing the
child. A shape that only filtered the child's own hits would leave a circular
button with a transparent icon padding unhittable between the glyph and the
rim. It clips nothing and opens no layer, unlike `RenderClipOval`, which also
hit-tests by shape. `harness_content_shape_rounded_rect_is_the_hit_area` pins
the hit through a child that ignores pointers;
`harness_content_shape_oval_rejects_the_corners_of_its_box` pins the corners.

`RenderHitSlop` does not hit-test its area itself; it reports the area and the
pipeline resolves it (flui-rendering ARCHITECTURE.md, "Expanded hit areas are
resolved after the visual walk"). Its area is the box grown by the insets, then
grown symmetrically to the minimum target per axis; a negative or non-finite
input counts as zero so the area never shrinks below the box. The minimum is a
`MinTarget`: an authored length, or the scope's `GestureSettingsProvider`,
read on every hit test so a live host update needs no rebuild (the provider is
owner-local data, not a callback). An absorbing `RenderAbsorbPointer` reports
`blocks_hit_slop`, so no area takes a point that landed on a barrier.
`harness_hit_slop_min_target_centres_on_the_child` pins the growth.

## Thread safety

No locks. Catalog objects are mutated on the UI runtime's layout/paint thread
through `PipelineOwner`; they hold no shared mutable state of their own.

---

## Friction log

| Item | Notes |
|------|-------|
| Shared `classify_cache_window` helper across list + grid | Grid owns its classify today; list uses `finite_leading_cache_edge`. Consolidating into one module is optional follow-up once a third consumer appears. |


### RenderImage source fitting

RenderImage paints through painting's `ImagePlacement` (ADR-0229), the same
contract decoration images use: any `BoxFit` at a fractional alignment with
optional repetition, or a nine-slice whose corners keep their logical size.
Fitting works on the natural size (intrinsic size over scale); the cropped
source converts back to decoded texels axis by axis before Canvas records an
ImageRegion (ADR-0115), so Cover paints inside the allocated box and high-DPI
cropping uses the decoded image's coordinates. A natural extent that
overflows `f64` (a tiny scale) is bounded with its ratio preserved, both in
layout and in placement. The placement is paint-only; layout never reads it.
The GPU consumers are `render_image_scaled_cover` and
`sliced_placement_keeps_logical_corners`, rows of the engine's
`painter_images_and_offscreen_results_read_back_as_specified` family.
`harness_image_extreme_scale_preserves_constrained_aspect_and_paint` pins the
bounded layout and recorded paint for every fit at tiny scales, and
`harness_image_placement_slices_and_repeats_in_logical_units` pins a mounted
2x slice and logical-size repetition.

### Superellipse clips and backdrop groups

`RenderClipRSuperellipse` hit-tests with `RSuperellipse::contains`, the same
normalized exponent-4 corner curve the engine's clip mask draws (ADR-0231);
`harness_clip_rsuperellipse_hit_tests_the_drawn_curve` pins a corner hit a
circular corner would reject. `RenderBackdropFilter` carries an optional
`BackdropGroupKey` onto its layer; grouped filters on one surface sample one
capture (ADR-0230), pinned by
`harness_backdrop_filter_carries_its_group_to_the_layer` and the engine row
`grouped_backdrops_share_one_capture`.

### Annotated regions and external textures

`RenderAnnotatedRegion` annotates its own box through a paint scope that becomes an
`AnnotatedRegionLayer`; readers use `LayerTree::find_annotations` (ADR-0232), pinned by
`harness_annotated_region_is_found_in_the_scene_over_its_box`. `RenderTexture` fills
the largest box its constraints allow and draws its texture there; a new frame
revision repaints it, which is how a frame fed through the presentation's
`TextureRegistry` reaches the screen without other changes (ADR-0233), pinned by
`harness_texture_fills_its_box_and_repaints_per_frame`.
