# Spec — #530 Unify lazy sliver child lifecycle, correction, and panic recovery

Status: draft v1 (2026-09-03). Author: autopilot session. Reference floor: `scratchpad/530/flutter-floor.md`.

## 1. Problem, verified in the tree (main @ 4f9c931f)

| Symptom (issue text) | Root cause found | Evidence |
|---|---|---|
| composite lazy child never materialises (2 nodes vs 5) | `SparseChildren::ensure` stamps `SliverMultiBoxAdaptorParentData.index` on the *top-level* render node at insert. A composite child has none; its first render descendant mounts later via `RenderBehavior::on_mount` → `adopt_render_child(parent_render_id, …)` with **no stamp**, so `walk_virtualizer_band` never sees it in `logical_to_slot`. Flutter stamps at `didAdoptChild` with slot inheritance through component elements. | `sparse_children.rs:305-327`, `behavior.rs:818-835`, `unified.rs:194-205` (parent_render_id pass-through), `lazy_list.rs::lazy_list_view_builder_settles_composite_children` (#817 pin) |
| children land one frame late; scroll pops in | `service_child_requests` runs **after** `run_frame` (paint) in both frame paths; the fixpoint that `LayoutBuilder` uses (`run_frame_with_layout_builders`, ADR-0017) never services lazy requests. ADR-0017 §Consequences names this as the deferred upgrade. | `bootstrap.rs:325-337`, `ui_realm.rs:2531-2536`, `owner/layout_builder.rs:386-425` |
| fixed-extent list is not lazy; eager grid keeps stale children | `RenderSliverFixedExtentList` lays out *every* attached child each frame with no band; `ListView::new`/`SliverFixedExtentList` mount all static children eagerly. `RenderSliverGrid` (static) same. Two ownership models coexist: element-owned request strategy (`RenderSliverList`, `RenderSliverGridLazy`) and render-owned closure strategy (`RenderSliverListLazy`, zero production constructors). | `sliver_fixed_extent_list.rs` (objects), `sliver_grid.rs`, manifest pin `sliver_fixed_extent_list_offscreen_children_are_not_built_on_initial_window_pin` |
| correction differs from Flutter | Two distinct things. (a) **Model:** FLUI keeps the first visible item pixel-stationary (anchor correction = growth of resident items *above the anchor*); Flutter keeps the first *retained* child's stale offset and lets growth of retained-but-invisible children shift visible content (192px in the oracle). FLUI's measured windows `[14,21],[12,18],[9,15]` are exactly the stationary-anchor prediction — not a bug. (b) **Bug:** `resolve_anchor_correction` suppresses emission while scrolling backward and only replays on a forward scroll, so a list scrolled back to its start never converges: FLUI ends at `[1,8]`, oracle `[0,6]`. | `virtualized_band.rs:136-160`, `sliver_list_correction_test.rs` module doc "Case 1", oracle arithmetic in this spec §5 |
| builder panics can escape | The delegate builder is invoked bare in `ChildManager::service` (`(self.builder)(logical_index)`); only the child's *own* `build()` is under `build_or_recover`. A panicking item builder unwinds through `service_child_requests` with the sliver half-serviced. | `sliver_adaptor.rs:377`, `behavior_commons.rs:68-140` |
| no public per-item key path; keyed reorder loses state | `SparseChildren` is index-keyed; `refresh_resident` reconciles by type only ("Sparse children never carry a key … today"). No `find_index_by_key`. Static-children lists can't express keys at all. | `sparse_children.rs:227-300`, `sliver_list.rs` (widgets) `forwarding_child_key` doc |
| paint/hit-test/semantics see all arena-resident children | eviction happens post-paint (see row 2); positioning only touches in-band slots, so an off-band child is painted at its stale offset for one frame. | `virtualized_band.rs` step 10 + `PaintCx::paint_children` (no gating) |

## 2. Design principles (what is better than Flutter, and why)

1. **One ownership model.** The element tree owns every lazy child; render objects request indices and report the retained band. Render-owned child sources are deleted (`RenderSliverListLazy`, `RenderSliverGridLazy`). Flutter has the same single model; FLUI additionally keeps *all* three adaptors on one engine so list/fixed/grid cannot drift.
2. **Same-frame materialisation, no reentrancy.** Lazy requests are serviced inside the bounded layout↔build fixpoint (ADR-0017's loop), between layout passes and before paint. Flutter needs unsafe `invokeLayoutCallback`; FLUI gets the same observable result (no blank first frame) from the existing borrow-safe loop. Cost: extra layout passes when the band changes, bounded by `MAX_LAYOUT_BUILD_PASSES`.
3. **Adopt-time index stamping via slot inheritance.** A `sliver_slot: Option<usize>` rides on `ElementCore` exactly like `parent_render_id` (set at insert from the parent, passed through composite elements, reset at render elements). `RenderBehavior::on_mount` stamps parent data whenever the slot is `Some`. This is Flutter's `didAdoptChild` + inherited slot, without a walk.
4. **Anchor-stationary correction, applied immediately.** Recorded as the improvement over Flutter's retained-stale-offset model (§5). The backward-scroll suppression is removed; drags compose with corrections by delta (verified at implementation).
5. **Key-first identity without a callback.** Resident children are reconciled by key inside the band when the delegate changes (learned key→element map), so keyed insert/remove/reorder preserves state with no `find_index_by_key`. An explicit `find_index_by_key` remains for cross-band moves (Flutter's only mechanism). List delegates derive the key map automatically (Flutter parity).
6. **Builder panic → `ErrorView` child, transactional.** The delegate call is under `catch_unwind`; a panic yields the error view for that index and nothing else changes.
7. **Committed band only.** Eviction happens before paint (principle 2), so paint/hit-test/semantics observe exactly the band the last converged layout committed.

## 3. Architecture

### 3.1 flui-rendering
- `SliverMultiBoxAdaptorParentData { index, layout_offset, cross_axis_offset, keep_alive }` becomes the single parent-data type for all three adaptors (`SliverGridParentData` is folded in; Flutter subclasses, FLUI needs one downcast target in `append_sparse_sliver_children`).
- No new pipeline sinks. `request_child_build` / `emit_retain_band` stay.

### 3.2 flui-objects — one engine, three extent models
```
sliver/multi_box/
  mod.rs        pub struct MultiBoxBand { logical_to_slot, attached, correction accumulator }
                pub(crate) trait ExtentModel {
                    fn sync_count(&mut self, n);
                    fn band(&self, c: &SliverConstraints) -> Band { first, last, cache_first, cache_last };
                    fn geometry_for(&self, i) -> ChildGeometry { layout_offset, cross_axis_offset, main_extent, box_constraints };
                    fn record_measured(&mut self, i, extent, anchor) -> Option<f32 /*correction*/>;
                    fn total_extent(&self, reached_end: Option<usize>) -> f32;
                }
                pub(crate) fn walk(model, band, constraints, ctx) -> (SliverGeometry, Band)
  variable.rs   VariableExtent(Virtualizer)   — today's walk_virtualizer_band semantics
  fixed.rs      FixedExtent { item_extent }   — Flutter sliver_fixed_extent_list.dart index math
  grid.rs       GridExtent { delegate, layout } — Flutter sliver_grid.dart index math
```
Render objects (public catalog, harness-tested): `RenderSliverList`, `RenderSliverFixedExtentList`, `RenderSliverGrid` — all request-strategy. Deleted: `RenderSliverListLazy`, `RenderSliverGridLazy`, `virtualized_band.rs` (moved), `OffBandDisposal`.

### 3.3 flui-view — one adaptor element
- `SliverChildDelegate` (object-safe): `build(index) -> Option<BoxedView>`, `estimated_child_count() -> Option<usize>`, `find_index_by_key(&dyn ViewKey) -> Option<usize>`, `should_rebuild(&dyn SliverChildDelegate) -> bool`, `did_finish_layout(first,last)`.
  Impls in flui-view: `BuilderDelegate` (fn + count + optional key callback), `ListDelegate` (`Vec<BoxedView>`, automatic key map).
- `SliverMultiBoxAdaptor<R>` view: `{ delegate: Rc<dyn SliverChildDelegate>, config: R::Config }` with `R: LazyMultiBoxRender` (the three render objects) — replaces `SliverList` + `SliverGridLazy` views; `SliverList`, `SliverFixedExtentList`, `SliverGrid` become thin typed wrappers/aliases.
- `SliverAdaptorBehavior<R>`: registers one `ChildManager`; `is_sparse_host() == true` (new `ElementBehavior` hook used by insert to seed `sliver_slot`).
- `SparseChildren` → `ResidentChildren`: `BTreeMap<usize, Resident { element, key: Option<Box<dyn ViewKey>> }>`; ops: `ensure`, `evict`, `retain_band`, `reconcile_with_delegate` (key-first), `stamp_first_render_descendants` (relocation path only).
- `ChildManager::service` gains the panic boundary and the key-first reconcile.
- `ElementCore`: `sliver_slot: Option<usize>`; `UnifiedElement::child_sliver_slot()` mirrors `child_render_id()`; `ElementTree::insert*` seeds it at the four `set_parent_render_id` sites.
- `RenderBehavior::on_mount`: after `adopt_render_child`, `if let Some(i) = core.sliver_slot() { stamp }`.
- `BuildOwner::run_frame_with_deferred_builds` (rename of `run_frame_with_layout_builders`): fixpoint pass = `run_layout → service_layout_builders || service_child_requests`; `service_child_requests` keeps its standalone signature for the post-frame call sites (which become no-ops on a converged frame).

### 3.4 flui-widgets
- `SliverChildBuilderDelegate` / `SliverChildListDelegate` (public) wrap the flui-view delegates and own the wrap chain (`RepaintBoundary` with forwarded key; semantic indexes later).
- `ListView::new(extent, children)` → `SliverFixedExtentList` over a list delegate (lazy). `ListView::builder` → `SliverList`. `GridView::count/extent` → `SliverGrid` over a list delegate; `GridView::builder` → `SliverGrid` over a builder delegate. `SliverList::{builder,list,separated}`, `SliverFixedExtentList::{builder,list}`, `SliverGrid::{builder,count,extent}` per Flutter's constructor set.

### 3.5 Frame paths
`flui-testing::bootstrap`, `HeadlessBinding::pump_frame`, `UiRealm::draw_frame` call the renamed fixpoint; the trailing `service_child_requests` call is deleted (bootstrap's contract "the bootstrap frame is the same frame pump_frame runs" is kept by construction).

## 4. Slices (each its own PR, merge on green)

1. **Same-frame + adopt-time stamp.** fixpoint change; `sliver_slot` propagation; on_mount stamp; un-ignore the #817 pin; a test that a `StatefulView` item mounts, updates, and disposes; frame-count test proving a scroll band change materialises in one pump.
2. **One engine.** `multi_box/` extent models; fixed-extent and grid on the request strategy; delete the render-owned types; list delegate; harness catalog + parity manifest updates; the fixed-extent pin goes green.
3. **Identity + recovery.** key-first reconcile; `find_index_by_key`; delegate panic boundary; tests: keyed insert/remove/reorder preserves `ViewState`, builder panic yields `ErrorView` and a valid tree, multiple simultaneous evictions.
4. **Correction.** remove backward suppression; verify drag/correction composition in `Scrollable`; ADR + mapping entry; replace the oracle pin with the FLUI oracle (stationary anchor, convergence to `[0,6]`); manifest `diverged` reclassification with the arithmetic.
5. **Close.** ROADMAP/TRACKER/Cross.H text, crate docs, issue close with evidence table.

## 5. Correction model — the arithmetic (oracle `slivers_test.dart` "inaccurate scroll offset")

Viewport 600, cache 250, items 96 (even) / 0 (odd) until the swap. After drag −750: offset 750, first visible = item 14 (672..768). Resident band 10..29. Swap → every item 96.

- **Flutter**: firstChild = item 10 keeps stale offset 480; the walk re-lays out 11,12,13,14… at 576, 672, 768, 864. Item 14 moves from screen −78 to +114 (+192, two grown odd items 11 and 13). Visible window `[12,19]`. Content jumps 192px with no user input.
- **FLUI (stationary anchor)**: anchor = item 14; resident odd items above it that grew: 11, 13 → correction +192 → offset 942; `offset_of(14)` = 7×96 + 192 = 864; screen −78, unchanged. Window `[14,21]` — the measured value. No jump.
- After the remaining drags the oracle's last window `[0,6]` is reachable only if corrections keep applying while scrolling backward; today's suppression stops at `[1,8]`. Removing it makes the final window match.

Divergent-by-decision windows: the three intermediate checkpoints. Replacement oracle: (1) anchor screen position invariant across the swap; (2) every window equals the accurate-offset prediction; (3) final window `[0,6]` with `pixels == 0`.

## 6. Out of scope (named, with owners)
- `KeepAlive` bucket / `AutomaticKeepAlive` — field kept, semantics later (new issue).
- Unknown item count (`childCount == null`, binary-search end discovery) — `Virtualizer` allocates per item; needs a growable model (new issue).
- `addSemanticIndexes` / `IndexedSemantics` — with #675 semantics work.
- Viewport commit model, `anchor`, `clip_behavior` — #537 (unblocked by this).

## 7. Risks
- Fixpoint pass count on a large jump (band → build → remeasure → new band): bounded; instrument with a tracing counter and a test that a 1000-item jump converges ≤ 4 passes.
- `sliver_slot` on GlobalKey relocation: relocated subtrees do not re-mount; `ensure` stamps first render descendants explicitly on that path, with a debug assertion in the walk that every attached slot has parent data.
- Deleting public render types is breaking (allowed per the goal); runtime-contract manifest and harness catalog must move in the same PR.

---

## 8. Adversarial review (2026-09-03) — verdict SURVIVES with required changes; what changed

| # | Finding | Disposition |
|---|---|---|
| 1 | **Blocker.** A band under an over-estimate converges geometrically (passes ≈ ln(A/W)/ln(1−A/E)); inside the 10-pass fixpoint that is a debug `BUG:` panic on user content. | **Fixed in slice 1.** (a) The hint for unmeasured items adapts to the *band-local* mean of measured children (`Virtualizer::measured_mean_in` over the band's measured, non-zero children — zero-extent placeholders excluded, or the `sliver_list_test` oracle's exact clamp breaks; `adapt_default_estimate` with an anchor-preserving correction; the walk re-queries and requests the widened band in the same pass when no correction was owed). (b) A separate lazy-band budget (`MAX_LAZY_BAND_PASSES = 6`, per-owner, test-overridable) stops servicing and defers to the post-frame safety net with a `warn!`; the `BUG:` path stays reserved for layout builders. Tests: 20× over-estimate settles in the bootstrap frame (red without (a)); deferral test with budget=1 (red with the knob ignored). |
| 2 | Suppression misdescribed: `resolve_anchor_correction` replays on any non-decreasing offset, so it cannot alone explain `[1,8]`; the untraced candidate is the anchor-is-the-measured-item rule (a zero-extent item that *is* `range.first`). | **Accepted.** Slice 4 is gated on an executed experiment with a per-pass `(offset, anchor, pending, emitted)` trace before any ADR. §5's swap-step arithmetic was independently confirmed. |
| 3 | Band-local key matching loses state on an insert at the band's head (the last resident key leaves the band); Flutter runs `findIndexByKey` first. Remap must restamp parent data, re-derive `sliver_slot` down the composite chain, and apply first-wins on duplicate keys. | **Accepted.** Slice 3: `find_index_by_key` (list delegate derives the map; builder delegate optional) runs first for every keyed resident; band-local matching is the fallback and the acceptance text says a builder delegate without the callback loses state on insert, as Flutter does. Remap = `node.slot` update + `recompute_subtree_ancestry` + `stamp_first_render_descendants`. |
| 4 | No uniqueness guard on the index → a stale index paints at a stale offset silently. | **Fixed in slice 1:** `debug_assert!` in both band walkers that no two attached slots carry one logical index. |
| 5 | `ExtentModel` cannot express the grid's walk-level results, bakes the list's correction into the trait, and promises an unknown count the engine cannot hold; the "gate list" in §7 was wrong (no runtime-contract entries; what breaks is the harness catalog, re-exports, manifest pins, two flui-rendering tests). | **Accepted → ALT-2.** Slice 2 unifies the *element* side (one adaptor element, one `ResidentChildren`, one `ChildManager`) and keeps three render objects with Flutter-faithful index math; a shared helper only where the code is literally identical (slot map, positioning, retain band). `estimated_child_count` stays `usize` until the growable count lands. |
| 6 | "Transactional" is per-item; three delegate call sites; `create_render_object` panics escape. | **Accepted.** Slice 3 wraps all three sites; docs say per-item recovery (as Flutter) and name the `on_mount` escape as a follow-up. |
| 7 | Per-pass `finalize_tree` verifies and clears the GlobalKey ledger up to ten times a frame. | **Fixed in slice 1:** in-loop servicing runs `unmount_inactive_elements` only (`service_child_requests_between_passes`); the post-frame call keeps the full finalize, so verification stays per frame. |
| 8 | Non-converged path paints a stale band; export passes-per-frame. | **Partly done:** `warn!` on budget exhaustion + `debug!(lazy_passes)` per frame; the exception is named in the frame-path comments. A counter export is a follow-up with the frame telemetry. |
| 9 | KeepAlive is a `ParentDataWidget` writing the same parent data as the adopt-time stamp. | **Recorded** for the KeepAlive follow-up issue. |

Process note: the review ran on a tree where slice 1 was already implemented (uncommitted); the reviewer was right to flag the skipped pre-code gate. Slices 2–4 get their review before code.

## 9. Slice-2 plan review (2026-09-03) — verdict SURVIVES with required changes; order changed

| # | Finding | Disposition |
|---|---|---|
| 1 | **Blocker.** A request-strategy sliver cannot observe a leading-insert failure (`request_child_build` always returns `Scheduled`), so Flutter's "leading insert failure → `scrollOffsetCorrection`" has no trigger; keying it on "leading index absent" teleports the viewport on every backward scroll. | **Accepted.** The fixed-extent object emits no correction. Contract: builder `None` at `i` ⇒ manager clamps the count to `i` (the grid manager's `clamp_render_item_count`, generalised) ⇒ next pass reports `i × extent` with the past-the-end guard ⇒ `apply_content_dimensions` clamps pixels. Recorded divergence: a non-monotone builder truncates at `i` where Flutter teleports and keeps higher children. Widget test: backward scroll on `ListView::new` is red if any correction is emitted. The list manager's silent `None` drop becomes the same clamp, with its own extent test. |
| 2 | **Blocker.** The eager grid gates paint/hit-test on a commit-last `laid_out_band`; the lazy grid and list walk every attached slot, which paints stale residents on the deferral path (budget exhausted → eviction next frame). The four eager pins cannot retarget without that gate. | **Accepted → ALT-1.** Promote the committed-band gate into the shared helper and apply it to all three objects *before* the rename; retarget the pins 1:1 with seeded residents and no manager. This also closes the deferral-path hole in principle 7. |
| 3 | **High.** Moving `ListView::new` / `GridView::count` onto `refresh_resident` (index-reconciled) loses keyed `ViewState` on reorder until slice 3. | **Accepted → reorder.** Slice 3 (key-first identity, `find_index_by_key`, remap restamp, panic boundary) lands before slice 2. Add a keyed `StatefulView` reorder test on `ListView::new` that is red without the remap. |
| 4 | Parent-data switch blast radius: ~20 harness/scaffold sites plus `flui-rendering/tests/sliver_fixed_extent_list.rs`, the harness self-test, `render_inspector.rs`. | **Accepted** into the slice-2 inventory. |
| 5 | Deleting `RenderSliverListLazy` leaves `build_and_layout_box_child` / `dispose_box_child` / `pending_builds` with no production consumer ("shipped seam never wired", by design). flui-rendering *does* dev-depend on flui-objects. `harness_snapshot.rs:431` is cited by ROADMAP as the Core exit verification. | **Decision owed → ALT-3, decided: delete the seam** with an ADR-0003 amendment (one ownership model, for real), retarget the ROADMAP citation to `lazy_list.rs`'s scroll tests, regenerate the trybuild stderr, sweep the doc mentions and `workspace-layers.toml`. |
| 6 | No unbounded-window guard for the fixed-extent list (Flutter loops until the builder returns null). | **Accepted:** port the lazy grid's `MAX_UNBOUNDED_WINDOW_CHILDREN` / sentinel and the past-the-end guard. |
| 7 | Generic view mechanics: manual `Debug`/`Clone`, keep the unconditional `LAYOUT` impact, keep config validation in the constructor (not `on_mount`). | **Accepted.** |
| 8 | Edge cases to record: `setDidUnderflow` look-ahead (pin: count n→n+1 while scrolled to the end grows the max extent in-frame); `hasVisualOverflow` formula choice; `semanticBounds` fallback has no hook (pre-existing, record as dropped); keep-alive honoured only in the deleted `RenderOwned` arm (KeepAlive follow-up). | **Accepted** into the ADR for slice 2. |
| 9 | Evidence plan could not fail for the right reasons; step order left `ListView::new` rendering nothing mid-way. | **Accepted:** widget-tier tests named in 1/2/3; land slice 2 as one PR. |

**Order now:** slice 4 (correction model, ADR-0051 — in flight on `feat/530-anchor-correction-model`) → slice 3 (identity + recovery) → slice 2 (unification with the band gate, clamp contract, seam deletion) → slice 5 (close).

## 10. Slice-3 plan review (2026-09-03) — verdict DOESN'T SURVIVE as written → ALT-1 adopted

| # | Finding | Disposition |
|---|---|---|
| 1 | In-place single-map remap orphans an element on any shift of ≥2 keyed residents (remove/insert overwrites), leaving it attached, stamped, unevictable; the walk's uniqueness assert then fires. Flutter's second map (`newChildren`) is load-bearing. | **Accepted → ALT-1:** two-phase reconcile. Snapshot `old: (idx, id, key)`; build every union index into `new` (each under the per-item catch); match `new → old` by `find_index_by_key`, then key-bucket + `key_eq` first-wins (band-local), then positional `can_update_by_id`; apply into a **fresh** `BTreeMap` (update / relocate+restamp / mount / evict unclaimed). |
| 2 | Band-local matching "before step 2" is impossible and loses all state on insert-at-head. | **Accepted:** part of ALT-1 (match after building all union views). |
| 3 | `RepaintBoundary::forwarding_child_key` double-registers a `GlobalKey` (boundary + item) → debug panic at mount; Flutter's `_SaltedValueKey` is load-bearing. Pre-existing; no test keys a lazy item today. | **Accepted:** `SaltedKey` (`is_global_key() == false`, `key_eq` on the inner key, `find_index_by_key` unwraps); test: `GlobalKey`'d item in `ListView::builder` mounts (red today). |
| 4 | `ErrorView` owns no render object → the recovery path violates slice 1's "a lazy child owns a render node": a phantom slot the sliver never lays out. | **Accepted:** `RenderErrorBox` in flui-objects; `ErrorElement` becomes a render element over it (serves #561 too). |
| 5 | Planned panic/reorder tests assert the wrong layer. | **Accepted:** assert an attached render child stamped `k` with non-zero measured extent and its paint; reorder test asserts each resident's parent-data index equals its map index and `retain_band` can evict every attached child (release-safe oracle). |
| 6 | An index-keyed error view collides with user `ValueKey<usize>` keys via `find_index_by_key`. | **Accepted:** the error view is unkeyed (Flutter's `_createErrorWidget` is too); it updates in place while the panic persists. |
| 7 | No `forgetChild`: a `GlobalKey` graft out of the list leaves a stale sparse entry that `retain_band`/`refresh_resident` later destroys or overwrites. | **Accepted:** `ChildManager::forget_child(ElementId)` driven from `retake_active_global_key` (mirrors `sliver.dart:1103-1108`). |
| 8 | Contract gaps: no `KeyId` type (use hash bucket + `key_eq`); `did_finish_layout` absent from the trait and the `(0, usize::MAX)` sentinel band must not be forwarded; `recompute_subtree_ancestry` private and its ordering with `node.slot = j` is load-bearing; "clear layout offset" is a no-op; duplicate `GlobalKey`s in one band are ADR-0050's verdict, not first-wins. | **Accepted:** one tree API `relocate_sparse_child(id, j)` (sets slot → recomputes ancestry → restamps, in that order); trait drops `did_finish_layout` for now; duplicates: first-wins for local keys, ADR-0050 for global; resident key read from `ElementNode::key()` (no duplicate storage). |
| 9 | Undocumented divergences: `_replaceMovedChildren` applied to all adaptors; `_didUnderflow` look-ahead covered structurally by the forced `LAYOUT` + same-frame re-request; `updateChild` offset preservation covered by the per-pass rewrite. | **Accepted:** three mapping entries in the slice-3 ADR/mapping section, with the look-ahead pin spec §9 #8 owes. |
| 10 | Retracted attacks: same-host moves never touch the registry/reservations; `AssertUnwindSafe` argument transfers; `should_rebuild` cost unchanged; `Rc::ptr_eq` on the builder is a free improvement. | Recorded; `BuilderDelegate::should_rebuild` uses `Rc::ptr_eq`. |

## 11. Slice 3 landed (2026-09-03) — ADR-0052
Implemented per §10/ALT-1: `SparseChildren::reconcile` (two-phase, fresh map, relocate + restamp),
`build_item_or_error` at all three call sites, `SaltedKey` on the repaint boundary (red test: the
duplicate-GlobalKey panic), `RenderErrorBox` + `ErrorView` as a render view (harness-catalogued),
`ChildManager::forget_child` from the active retake (red test: grafted element destroyed without it),
`ListView/GridView::find_index_by_key`. Unit tests: shift-of-two and swap keep both elements.
**Gap found and recorded:** a `GlobalKey`'d descendant of a removed unkeyed subtree is unmounted
immediately (no deactivate-then-retake before finalize) — tree-wide, dense included; the graft test
therefore runs without the per-item boundary. Candidate for the GlobalKey follow-up issue.

**Status 2026-09-03 (late):** slice 1 merged (#824 → d084ec55), slice 4 merged (#826 → dae32d29),
slice 3 open as #828 (gate green, awaiting bots + CI). Slice 2 plan v2 under adversarial review.

## 12. Slice-2 plan v2 review (2026-09-03) — verdict DOESN'T SURVIVE as written → v3

| # | Finding | Disposition |
|---|---|---|
| 1 | The per-object committed-band gate cannot be written in flui-objects: `PaintCx` exposes `child_count`/`paint_child` only, `SliverHitTestContext` neither parent data nor a child count; slot ≠ index for the lazy objects. | **Accepted → ALT-2** (evict-before-paint on the deferral path: when the lazy budget trips, apply the last pass's retain band — evict only — before the final `run_frame`), plus a live `ctx.child_count()` for hit-test instead of the `attached_child_count` snapshot. ALT-1 (a pipeline-level "positioned this pass" generation stamp read by paint, hit-test and semantics) is recorded as the class fix for a follow-up ADR; it is a pipeline-wide contract with its own sweep. |
| 2 | Step 4a' skips laying out already-attached children inside the widened band, then positions them with a stale size. | **Accepted:** 4a' lays out attached widened-band children exactly as step 4 does, feeding `set_measured`. |
| 3 | `attached_child_count` snapshot is inconsistent on the deferral path. | **Accepted:** live count (see #1). |
| 4 | Semantics is assembled from `node.children()` unconditionally; no gate reaches it. | **Accepted:** ALT-2 makes stale residents impossible at assembly time on the deferral path; recorded as the reason ALT-1 is the eventual shape. |
| 5 | Budget-1 deferral test cannot go red. | **Accepted:** budget 0 + a scroll jump past the residents; assert no hit at the old rows' rect and no paint of them. |
| 6 | Deleting the seam leaves `pipeline/deferred.rs`, `defer_*`, `apply_deferred_mutation`, `LogicalIndexParentData` and the port-check entry with zero producers. | **Accepted:** delete them together (tests in `attach_detach_lifecycle.rs` and `deferred.rs` retargeted/removed). |
| 7 | Inventory: grid's resident layout also calls `build_and_layout_box_child` (→ `layout_box_child`); removing `Ready(G)` changes `request_child_build`'s return type (public); 9 tests not 12; two sliver-level benches; `subtree_arena.rs` `!Send` rationale moves. | **Accepted** into the v3 inventory. |
| 8 | ADR-0003 amendment must say: adopting rejected alternative (c) knowingly; mid-pass re-entry becomes a breaking change. | **Accepted.** |
| 9 | Fixed-extent must emit the retain band (incl. zero-count and past-the-end arms) and is covered by ALT-2 like the others. | **Accepted.** |
| 10 | `SliverList::list` exists — extend it with the key map; `should_rebuild = !Rc::ptr_eq`; the list delegate's render update must not force `LAYOUT`. | **Accepted** (revises §9 #7 for identity-comparable delegates). |
| 11 | Record: shrink-wrap in an unbounded parent materialises all N (Flutter-equal); `usize::MAX` count with a finite window yields an absurd finite extent where Flutter binary-searches the end. | **Accepted** into the ADR; the manifest pin's scope says so. |
| 12 | "No correction" tests are tautologies; assert the clamp chain's end (`pixels` clamp after builder `None`). | **Accepted.** |
| 13 | Underflow pin, `not_parent_data.stderr`, `SliverList::separated/list` survive the wrapper, `laid_out_band` diagnostics name, band convention (half-open). | **Accepted.** |
| 14 | Step 6 casualties: `scroll_controller_test.rs:129` (stale captured child id) goes red; the fixed-extent pin flips green; two suspects in `grid_view_interaction_test.rs`. | **Accepted** into the gate list with dispositions. |
