# ADR-0227: Collection membership and platform totals

Status: Accepted

## Context

AccessKit 0.25.1 places `position_in_set` on a member and `size_of_set` on its
container. AccessKit consumer 0.39.1's `size_of_set_from_container` walks included
ancestors without stopping at an unknown List/Grid. Its common filter skips
GenericContainer. Publishing a total on each item therefore supplies no native
total, while blindly moving it above unknown or separately numbered sets can
announce an unrelated count.

## Decision

`SemanticSetMapping` remains the combined authority for membership, composed
offset and declared member count. The generic lazy adaptor installs that count
on its render host through a required `LazyMultiBoxRender` method, independently
of resident slots. Unknown declarations remain unknown after layout probes.

List, fixed-extent list and grid hosts describe actual List/Grid semantics
boundaries with explicit children. The member's position stays in parent data;
its automatic origin names the host's generational RenderId. Explicit authored
indexes take precedence after configuration folding. `SemanticsCollection`
declares a set independently of the materialized children, and provenance
distinguishes authored and lazy declarations.

An authored index belongs to the nearest authored declaration. Automatic
membership must match its nearest collection host. If an unknown or alternate
set would inherit an unrelated managed total, assembly withholds that total
from the platform projection. It keeps the original declaration and never
fabricates zero or an unbounded sentinel. This can withhold a known outer total
in the affected branch; it is an explicit native representation limit, not full
support for independently reset unknown counts. Raw authored scroll metadata
outside managed collections is not rewritten.

Collection-sensitive changes reassemble the containment context before
publication. A local graft remains available when neither its ancestors nor its
replaced/new subtree carries collection metadata. Declined grafts preserve the
existing staged callback custody, atomic identity allocation and publication
retry policy. Metadata reconciliation invokes no user code.

The obsolete slot/per-child total fields are removed. Custom lazy render
implementors must implement `set_semantic_set_size`; slot authors supply only
logical and semantic positions. This is a deliberate pre-1.0 public API change.

## Evidence

The widget rows
`lazy_collection_totals_reach_the_real_consumer_after_incremental_updates`,
`collection_consumer_preserves_unknown_and_explicit_nested_sets` and
`collection_consumer_uses_member_totals_for_separated_and_composed_delegates`
feed actual callback packets into `accesskit_consumer::Tree`. Assertions call
its native `size_of_set_from_container(common_filter)`, rather than checking a
stored item property. Cases include moving sparse bands, changed totals, empty
lists, separators, independent/composed delegates, nested known/unknown sets,
explicit alternate indexes and explicitly authored collections.
