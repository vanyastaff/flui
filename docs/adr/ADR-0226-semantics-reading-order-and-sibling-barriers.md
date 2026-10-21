# ADR-0226: Semantics reading order and sibling barriers

Status: Accepted

## Context

Paint order, accessible reading order and action blocking express different
policies. A mask may hide previously presented content while leaving its own
controls accessible. A reading-order key changes traversal without changing
layout or stacking. Span labels change speech without changing shaped text.

## Decision

`SemanticsConfiguration` carries the preceding-sibling barrier as an assembly
directive. `Semantics::block_previous_siblings` authors it through the existing
annotation object; a second box proxy with identical layout is unnecessary.
Barriers remove preceding fragments in presentation order, cross transparent
render wrappers and stop at a formed semantics boundary. Descendant action
blocking and exclusion retain their separate contracts. Discarded callbacks
remain under the assembly operation's ADR-0127 retirement policy.

Custom-painter regions use the same directive in background/child/foreground
presentation order. A foreground region can cover the render child; a background
region leaves later child contributions available. Repainting reevaluates the
directive without requiring a widget rebuild.

After barrier resolution, assembly stably sorts sibling fragments before
configuration folding. Keyed contributions precede unkeyed ones. Group names
sort lexically, with the unnamed group first; numeric orders sort within a
group. Equal orders, including signed zero, retain declarative order. Non-finite
orders refuse the draft before publication. A graft whose own key changed, or
whose barrier would affect siblings outside it, falls back to full assembly.

Spoken text uses each span's explicit semantics label instead of its own text,
then preserves the span's children. Empty overrides silence only that span's
own contribution. Label-only edits invalidate accessibility even when shaped
geometry is retained. Paint and text-selection offsets still use authored text.

## Evidence

The public widget rows
`rich_text_accessible_labels_follow_span_overrides_and_replacement`,
`semantics_sort_keys_reorder_published_siblings_after_local_updates` and
`a_semantics_barrier_crosses_proxies_but_stops_at_its_boundary` exercise actual
publication and replacement. The barrier row also refuses a platform click on
covered content. Rendering's
`harness_custom_paint_region_barriers_follow_painter_placement_and_updates`
checks both painter placements and notifier-driven changes. Rendering's
`nonfinite_reading_order_is_refused_before_publication` pins invalid numeric
input. These are targeted contracts, not a claim of full native text navigation.
