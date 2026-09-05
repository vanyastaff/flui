# #837 — semantic indexes for lazy sliver children

## What is actually missing (verified, and it is less than the issue implies)

The whole pipeline already exists and is tested:

| piece | where | state |
|---|---|---|
| `SemanticsNodeData::index_in_parent` (zero-based) | `flui-semantics/src/update.rs:123` | ships |
| `set_index_in_parent` on the configuration | `flui-rendering` | ships, pinned by `semantics_assembly.rs:1820` |
| one-based `position_in_set` + `size_of_set` at the boundary | `flui-semantics/src/accesskit_translation.rs:387` | ships |
| `IndexedSemantics` view + `RenderIndexedSemantics` | `flui-widgets/src/semantics/mod.rs:406`, `flui-objects/src/proxy/semantics.rs:223` | ships |

**Nothing wires them for lazy children.** No delegate wraps an item, and
`IndexedSemantics`'s own doc says so outright: *"in the reference every lazy
sliver delegate wraps every materialised item in one by default. FLUI's do
not."* This is the repo's named dominant defect class — correct, tested code
that no production path calls — so the work is wiring, not building.

## Shape

Flutter's three knobs on `SliverChildDelegate`:

- `addSemanticIndexes: bool = true`
- `semanticIndexOffset: int = 0`
- `semanticIndexCallback: (Widget, int) -> int?` — `null` means "not a member
  of the set", which is how separators are excluded.

The callback is the one that matters and is easy to skip: `SliverList.separated`
supplies `(_, i) => i.isEven ? i ~/ 2 : null`, so a five-item separated list
announces "3 of 5", not "5 of 9". Without it, separators are counted — a defect
already seen once in this repo (Codex flagged "separators announced as set
members" on an earlier PR).

## The layering question, settled

`IndexedSemantics` is in flui-widgets, but `SliverList::separated` is an
inherent constructor in **flui-view** (`element/sliver_adaptor.rs:1262`), which
cannot depend on flui-widgets. It *can* depend on flui-objects
(`flui-view/Cargo.toml:30`), where `RenderIndexedSemantics` lives — so
flui-view wraps with a thin view over that render object rather than reaching
up a layer. Wrapping only in flui-widgets would leave `SliverList::separated`
unindexed, which is exactly the constructor that most needs it.

## The wrapper-order question, settled

Flutter wraps `IndexedSemantics(index, RepaintBoundary(item))` — the index node
is OUTERMOST (`SliverChildBuilderDelegate.build`: repaint boundary first, then
semantics). FLUI cannot copy that naively, because
`wrap_in_repaint_boundary` puts the item's key on the boundary via
`salting_child_key()`, and the sliver reconciles children by the key its
*outermost* child carries. An `IndexedSemantics` on top would hide it and cost
every item its state on insert, remove and reorder — the exact defect that
comment exists to prevent.

Two ways out, and the second is right:

1. Nest the other way — `RepaintBoundary(IndexedSemantics(item))`. Keeps the key
   where it is, but puts the index on an inner node. Whether the screen reader
   still sees it depends on the boundary being semantically transparent, which
   is true today and is not a contract.
2. **Move the salt to `IndexedSemantics`** when it is the outermost wrapper.
   It is a render view (`RenderIndexedSemantics`), so it can own a render node
   and carry a salted key — the very property `wrap_in_repaint_boundary`'s doc
   says a stateless `KeyedSubtree` equivalent lacks. This keeps Flutter's order
   and keeps the key visible.

Take (2). It needs `IndexedSemantics` to gain the same `salting_child_key`
affordance `RepaintBoundary` has, and the wrap helper to salt whichever wrapper
ends up outermost — not both, or the key is salted twice.

`RenderIndexedSemantics::describe_semantics_configuration` sets
`index_in_parent` on its OWN node, which is what makes the order matter at all.

## Order

1. Thread `semantic_index_offset` + `semantic_index_callback` through the
   adaptor config, defaulting to offset 0 and the identity callback.
2. Wrap at the same seam `wrap_in_repaint_boundary` uses — one wrapper per
   materialised item, skipped when the callback answers `None`.
3. `separated` supplies the even/odd callback.
4. Tests: a plain list announces `index_in_parent == logical index`; an offset
   shifts it; a **separated** list gives separators no index and items
   consecutive ones. The separated case is the one that fails if the callback
   is skipped, so write it first.
5. Oracle: `'SliverFixedExtentList.builder should respect semanticIndexOffset'`
   in `slivers_test.dart`, plus the `list_view_test.dart` semantics cases.

## Trap

The existing `## Mapping decisions` entry in flui-rendering records that FLUI
delegates deliberately do NOT index. That entry becomes false when this lands
and must be rewritten in the same change — not left to contradict the code, the
way three comments did in #883.


## Open fork: one node per item, or none — MEASURED, NOT DECIDED

The parity-shaped implementation (an `IndexedSemantics` wrapper per materialised
item, as Flutter does) costs **exactly one extra render node per item**.
Measured on `sliver_grid_delegate_negative_main_axis_extent_recovers_from_an_internal_assert_trip`,
whose whole assertion is a render-node count: 5 → 6 for a single materialised
child. Every lazy item already carries a `RepaintBoundary`; this makes two
wrappers per item, on the path whose entire purpose is to keep per-item cost
down.

**There may be no need for a wrapper at all.** The sliver already stamps each
child's logical index into `SliverMultiBoxAdaptorParentData.index` at `ensure`
time. If the semantics walk read that when the parent is a sparse host, the
index would reach the node with **zero** extra elements or render objects, and
it could not drift from the real index — the wrapper's value can, since it is
recomputed by a delegate that might disagree.

What the wrapper still buys, and would have to move somewhere:

- the callback's ability to DECLINE a child (separators), which is the whole
  reason the callback exists;
- `semantic_index_offset`.

Both are per-*sliver* configuration, not per-item, so they would sit on the
adaptor rather than on 100 wrappers.

**Do not ship the parity version by default.** It is the more expensive of two
designs and it is only the obvious one because Flutter does it that way, which
the Prime Directive explicitly says is a sufficient reason only when nothing
better is known. Something better is plausibly known here. Settle the fork —
the semantics-walk route needs checking against `build_semantics_fragments` and
the sparse-host detection that `hosts_sparse_children` now provides — before
wiring the delegates.

## Status of the branch

`feat/837-semantic-indexes` carries the parity-shaped implementation:
`IndexedSemantics` gained a `salting` affordance (needed under EITHER design if
a wrapper is ever outermost), a `SemanticIndexing` config type, `wrap_item`, and
two tests — one pinning that the salt comes from the item and not the boundary
(red against the naive derivation), one pinning that a declined child gets no
index node. Those parts survive the fork. The delegate wiring does not, and is
what the fork decides.
