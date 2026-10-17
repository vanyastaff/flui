# ADR-0234: Layout baselines compose through committed child offsets

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md)

## Context

A baseline-aligned parent needs its child's layout line even when sizing,
painting or pointer wrappers sit between it and the text. Dry queries alone
cannot supply that line: they describe hypothetical constraints and may be
unsupported for a layout-time builder. A fixed forwarding-depth limit also
changes alignment solely because an otherwise valid tree is deeper.

## Decision

The rendering pipeline resolves `forwards_baseline_to_only_child` against its
canonical tree. Each forwarded edge adds the child's committed vertical layout
offset. Traversal is bounded by the arena's node count, which admits every valid
chain and terminates malformed cycles. Non-finite composed results provide no
baseline. Paint-only transforms do not alter a layout line.

Single-child sizing and positioning objects opt into this walk instead of
adding another cached copy of the child's baseline. Their dry baseline methods
still derive the child's constraints and offset from the queried constraints.
An origin-preserving wrapper can forward both methods through the shared
baseline macro independently of its childless sizing policy.

Multi-child objects select the line their layout exposes. Stack reports the
highest natural line after applying positioned and aligned child offsets;
its live and dry queries share the same child-offset calculation. Wrap reports the
highest positioned real child baseline, with run formation, direction and
alignment shared between layout and dry queries. Theater reports the highest
onstage child baseline and clears it when no onstage child reports one.
Offstage forwards only while visible. LayoutBuilder forwards its committed
child's live baseline while retaining its prohibition on speculative dry work.
Flow and custom multi-child delegates do not acquire a baseline by default.

An explicitly authored `RenderBaseline` defines its configured same-kind line
whenever it has a child, including a plain box with no natural text baseline.
Other kinds derive from the child's actual line and placement; a childless box
has none. Dry queries follow the same policy. This preserves the table's
existing explicit-baseline contract. Flutter's natural-baseline-only policy for
that wrapper is a different choice; it is not adopted implicitly.

## Alternatives and consequences

Eager per-wrapper baseline caches duplicate tree offsets and fallible child
queries. They remain useful for objects that select among several children or
define their own line, but are unnecessary for ordinary one-child forwarding.
Answering only dry queries leaves live parents misaligned. A fixed hop bound
rejects valid nesting rather than proving acyclicity.

Stack, Wrap and Theater record their selected lines during layout because their live
query has no child context. Theater consequently contains fractional runtime
measurements and no longer implements `Eq`.

## Evidence

The public objects harness covers these contracts through
`harness_stack_baselines_include_positioned_and_aligned_children`,
`harness_single_child_layouts_preserve_live_and_dry_baselines`,
`harness_deep_proxy_nesting_preserves_live_baselines`,
`harness_wrap_reports_its_highest_positioned_baseline`,
`harness_theater_baselines_follow_only_onstage_children` and
`harness_offstage_and_layout_builder_forward_live_baselines`.
`harness_positioned_baselines_refuse_unrepresentable_sums` keeps Stack, Wrap
and IndexedStack from reporting an infinite composed line when both the
child's natural distance and its offset are finite.
`harness_table_baseline_alignment_lines_up_cells_on_their_shared_baseline`
pins the authored-line policy; the cross-kind and relayout cases live in
`harness_baseline_dry_baseline_recomputes_per_kind_offsets_after_relayout`.

The checked reference obligations come from Flutter stable
`abaf9c523780a608bd46686fd5e53740a07077f8`:
[shifted boxes](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/rendering/shifted_box.dart),
[Wrap](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/rendering/wrap.dart),
[Stack](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/rendering/stack.dart),
and [LayoutBuilder](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/widgets/layout_builder.dart).
