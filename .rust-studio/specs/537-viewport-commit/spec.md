# #537 v3 — degraded passes publish no dimensions; Flutter's center; anchor; clips

Supersedes spec-v2.md after its review (2026-09-04). What changed: D1 no longer aborts a pass
(a panic poisons on the first failure and clears the viewport's own `NEEDS_LAYOUT`, the poison skip
later serves the stand-in through the `Ok` arm with no flag, and children commit their own geometry
before the viewport decides — an abort left new child geometry against old offsets). Order of work
follows the review: D3 → D1 → D2 → D4/D5.

## D3 (first PR, landed in #841) — logical offsets resolved once at commit
- `layout_child_sequence` records `(slot, layout_offset, growth_direction, paint_extent)` per child
  instead of positioning immediately; after the loop accepts and the final size is known, one loop
  positions every child (`compute_absolute_paint_offset`). Nothing reads positions mid-loop
  (`position_child` writes only the transient slot; `try_cached_sliver_geometry` reads seeded
  constraints/geometry). The shrink-wrapping viewport's second `attempt_layout` exists only so
  reverse physical axes (`BottomToTop`/`RightToLeft`) get offsets computed against the final size;
  with deferred positioning it is deleted.
- Pins: BEFORE deleting the pass, add the reverse-axis shrink-wrap offset pin (no test exercises a
  `BottomToTop`/`RightToLeft` shrink-wrap today — zero hits in the harness and widget tests); then
  the counting pin (a counting sliver laid out N times for N passes, not N+1). All existing
  shrink-wrap numbers unchanged.

## D1 (second PR, landed) — a degraded pass commits its geometry but publishes no dimensions
- Arena: a per-walk `degradation_events: Cell<u64>` incremented when a child failure is caught and
  turned into a stand-in (`subtree_arena.rs` ~1047-1164) and when the poison skip serves a stand-in
  (~1593-1611, box twin ~804-823). Each `BoxLayoutContext` / `SliverLayoutContext` captures the
  counter at construction; `descendant_layout_degraded()` = counter moved since. The walk is
  depth-first and synchronous, so anything counted between a node's context creation and its query
  happened inside that node's subtree — any depth, not one level.
- Both viewports: lay out and POSITION as today (the tree stays internally consistent — every offset
  and every child geometry come from this pass), but when `descendant_layout_degraded()` is true
  after the accepted attempt: skip `correct_by`/`apply_viewport_dimension`/`apply_content_dimensions`
  for this frame (the position keeps its last published dimensions and its pixels), keep
  `min/max_scroll_extent` etc. as computed (they describe the degraded content honestly), and log
  once per degradation. The size stays `constraints.biggest()` (`RenderViewport`) /
  `constrain(shrink_wrap_extent)` under the CURRENT constraints (shrink-wrap) — never a stored size a
  new constraint might reject.
- Pin (red today: reads 200): 400 px adapter + a harness sliver that panics on its second layout +
  400 px adapter, `ScrollPosition::new(500.0)`; pump 1 accepts; pump 2 (panic) → `pixels() == 500`,
  the panicking sliver's layout counter == 2, the third adapter laid out in frame 2 (its counter);
  pump 3 under unchanged constraints (poison skip serves the stand-in) → still 500, and the
  degraded-pass flag observed. A second pin: a `SliverToBoxAdapter` over a `Text` whose layout panics
  (the depth case) — the viewport still sees the degradation. Recorded: while a descendant is
  degraded the scroll range is frozen; a recovered child (changed constraints → retry succeeds)
  resumes publishing.
- Also: `child_count` is read once per layout and bounds hit-test — unchanged here (#834 retires it).

## D2 (third PR) — Flutter's center model (breaking) + anchor (landed)
- Rename `center_sliver_index` → `center` (`set_center(Option<usize>)`, `center()`), Flutter's
  semantics: `center` is the first FORWARD child (default 0 = `children.first`; `None` = 0), the
  reverse group is the prefix `[0, center)` walked backwards and laid out first, its correction
  negated; `Some(n) == child_count` is INVALID (debug assert; release clamps to `child_count-1` and
  warns) — Flutter's center is always a child. Six test call sites change meaning and are rewritten:
  `render_viewport.rs` (`…axis_and_growth_matrix` :705, `…reverse_growth_to_slivers` :791,
  `…forward_then_reverse` :830, `…negative_min_scroll_extent` :869 — an all-reverse viewport is
  unrepresentable under Flutter and becomes a one-reverse-one-forward tree — ,
  `…center_at_child_count_behaves_like_no_center` :891 → deleted, its state does not exist),
  `sliver_hit_direction_matrix.rs:210`, `harness_viewport_reverse_group_overlap_is_always_zero`.
- `layout_child_sequence` gets a direction (`(0..center).rev()` for the reverse group); the
  obstruction bookkeeping (`sliver_obstruction_extents` pushed in layout order,
  `max_scroll_obstruction_extent_before(i)` summing `.take(i)`) is re-keyed by slot so the persistent
  header's pin (`render_object_harness.rs` ~11339) keeps its numbers; Flutter walks from center
  outward per growth direction (`viewport.dart` ~1904-1926).
- `RenderSliver::center_offset_adjustment` exists unwired: WIRE it (`offset.pixels() +
  center.center_offset_adjustment()` in the loop, `viewport.dart` ~1718/1727) — one addition, and a
  shipped seam nothing calls otherwise.
- Formulas: `_attemptLayout` verbatim (`viewport.dart` ~1781-1845; reviewed line for line).
  `anchor: f32` (0..=1): `centerOffset = main*anchor - correctedOffset`; content dimensions with the
  anchor terms.
- Widget layer: `Viewport::{anchor, center, clip_behavior, paint_order}` and the same on
  `CustomScrollView` (index-based `center`; a key-based one is a follow-up); setters `NONE` on
  equality (today `Scrollable` rebuilds per pixel — #833 — so every setter runs each rebuild).
- Oracles: `'Viewport anchor test'` (positions + `geometry.visible`, four offsets); the paint-order
  group needs a per-sliver paint log (a test sliver whose `paint` pushes its id into a shared log)
  and the hit-test-order group a sliver that adds itself and returns `false` (`add_self` exists) —
  confirm `HitTestResult` exposes entry order (lead); both `firstIsTop` and `lastIsTop`.

## D4/D5 (fourth PR) — clips
- `clip_behavior: Clip` on both viewports; paint clips only when `has_visual_overflow && clip !=
  None` (not calling `with_clip_rect` under `None` is the seam: `scope_layer` builds a `ClipRectLayer`
  for any mode); the wgpu backend ignores the mode (`backend.rs` `push_clip_rect(_, _clip)`) —
  ADR-0054 says the clipped modes are indistinguishable on screen today; the readback criterion is
  met for none-vs-clipped only, per primitive in flui-engine's suite.
- Hooks: `describe_approximate_paint_clip(child_slot, child_constraints) -> Option<Rect>` and
  `describe_semantics_clip(child_slot, child_constraints) -> Option<Rect>` on `RenderBox` (default
  `None`); the assembler holds the child's committed constraints (`RenderState::constraints()`) and
  passes them — the overlap-shrunk branch (`viewport.dart` ~912-934) is ported with them.
- Assembler: clips ride `SemanticsAssemblyContext` in both the full walk and `assembly_inputs_for`
  (and the graft's "same context ⇒ reuse" rule, or a scroll that changes only the clip grafts a
  stale hidden flag); intersect with the semantics clip (empty → dropped), `hidden` when fully
  outside the paint clip (`object.dart` `_SemanticsGeometry`).
- Oracles: the four Flutter persistent-header semantics cases currently DEFERRED in
  `sliver_persistent_header_test.rs`'s ledger (hidden=false in viewport, hidden=false partially off,
  hidden=true within the cache, absent beyond the cache) via `a11y_tree()`, `raw().is_hidden()`,
  `raw().bounds()`; the structural clip-layer oracle (`Clip::None` half red today).

## D6 — recorded
Keep Flutter's infinite-window model (ADR-0053 floor); ADR-0054 records the override of the
issue's wording. The widget-tier unbounded shrink-wrap pins for list and grid are green-today
regression coverage, labelled as such.

## Record
ADR-0054 "The viewport commits one result" — D1's degraded-pass rule, D2's model change and the
wired `center_offset_adjustment`, D3, D4's clip-mode gap, D5's two clips, D6.
