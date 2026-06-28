# U4.2 Request-Seam — Follow-Up Fixes (3rd commit)

Branch: `core2-lazy-slivers`
Prior commits: `3454ad50` (U4.2 impl), `a7fa68d1` (U4.2 polish)

## Scope

Three review-required fixes on top of the U4.2 implementation; all in a single
follow-up commit. Do NOT touch `crates/flui-view/src/element/sparse_children.rs`
(U4.3 scope).

## Fix 1 — Invariant guards in free functions

`crates/flui-objects/src/sliver/virtualized_band.rs`

The private `calc_paint_offset` and `calc_cache_offset` free functions mirror
`RenderSliver::calculate_paint_offset` / `calculate_cache_offset` from
`crates/flui-rendering/src/traits/render_sliver.rs` (lines 124, 147). The trait
methods both have `debug_assert!(from <= to);` as their first statement. The
extraction dropped these guards. Restore them as the first line of each function.

## Fix 2 — Non-exhaustive wildcard comment

`crates/flui-objects/src/sliver/virtualized_band.rs` (line ~290)

The `_ => {}` wildcard arm in the `match result` block inside
`walk_virtualizer_band` lost its explanatory comment during the U4.2 extraction.
Restore: `// ChildLayout is #[non_exhaustive]; forward-compat wildcard.`

## Fix 3 — Harness tests for RenderSliverList (BLOCKING)

### Infrastructure additions

`crates/flui-rendering/src/testing/parent_data.rs`:
- Add `SliverMultiBoxAdaptorParentData` to imports.
- Add `SliverMultiBoxAdaptor(SliverMultiBoxAdaptorParentData)` variant.
- Add `to_box()` arm.

`crates/flui-rendering/src/testing/tree.rs`:
- Add `SliverMultiBoxAdaptorParentData` to imports.
- Add `with_sliver_multi_box_parent_data(data) -> Self` convenience method.

### Four new harness tests

`crates/flui-objects/tests/render_object_harness.rs`:

(a) `harness_sliver_list_seeded_residents_laid_out_at_expected_offsets`:
Pre-seed items 0 and 1 at logical indices 0 and 1 (48 px each). Assert
`run.offset(item0_id).dy == px(0.0)` and `run.offset(item1_id).dy == px(48.0)`.
Assert only index 2 is in `take_pending_child_requests()`.

(b) `harness_sliver_list_off_band_resident_enqueued_for_removal`:
Pre-seed item 0 (48 px) at logical index 0. Mount with scroll=300 so cache
starts at 50 px > item 0's extent (48 px). After layout, item 0 is disposed:
`run.try_box_geometry(item0_id).is_none()`.

(c) `harness_sliver_list_scroll_extent_equals_virtualizer_estimate`:
3-item list, 48 px estimate, no residents. `scroll_extent == 144.0`.

(d) `harness_sliver_list_anchor_correction_forward_emits_backward_suppresses`:
Two-pass test. Pass 1 (scroll=100, item 0 = 60 px): forward scroll emits
`scroll_offset_correction = Some(12.0)`. Pass 2 (item 0 grown to 84 px,
scroll=72, backward): suppresses → `scroll_offset_correction = None`.

## Gate commands

```
CARGO_INCREMENTAL=0 cargo test -p flui-rendering -p flui-objects
CARGO_INCREMENTAL=0 cargo fmt -p flui-rendering -p flui-objects -- --check
CARGO_INCREMENTAL=0 cargo clippy -p flui-rendering -p flui-objects --all-targets -- -D warnings
```

## Out of scope

- U4.3 `LazySliverElement` + `sparse_children.rs` binding layer.
- Renaming `ChildLayout::Scheduled` or extending the non-exhaustive enum.
