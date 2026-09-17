# Flutter fixed-extent non-finite sliver lesson

Date: 2026-09-14

External signal:

- flutter/flutter#105630: `Unsupported operation: Infinity or NaN toInt` in
  `RenderSliverFixedExtentBoxAdaptor`.
- Comments show a production/release crash family seen in PageView,
  CustomScrollView, TabBarView and grid contexts, often without enough widget
  branch context to diagnose the bad sliver.

FLUI mapping:

- `crates/flui-objects/src/sliver/sliver_fixed_extent_list.rs` rejects
  non-finite or non-positive `item_extent`, which removes one Flutter failure
  source.
- `min_child_index_for_scroll_offset`, `max_child_index_for_scroll_offset`,
  and `float_to_index` still trust scroll-window inputs.
- Rust float-to-int casts are saturating rather than throwing:
  `NaN -> 0`, `+Inf -> usize::MAX`, `-Inf -> 0`.
- `RenderSliverGrid` already documents this exact hazard for infinite trailing
  cache windows, but fixed-extent list has no equivalent leading-edge guard.

Created:

- FLUI #1093: `sliver: guard fixed-extent index math against non-finite scroll windows`

Verification:

- Existing fixed-extent tests:
  `cargo nextest run -p flui-objects --lib -E 'test(sliver_fixed_extent_list)' --no-fail-fast`
  => 14 passed / 525 skipped.
- Existing issue search found no duplicate for fixed-extent non-finite sliver
  index math.

