# 837 — semantic indexes for lazy sliver children

## The design is already decided, and it beats the reference

`crates/flui-rendering/ARCHITECTURE.md` §"The set POSITION is published; the set
size waits, and the delegates do not wrap" settles the approach and records why:

- Flutter's lazy delegates wrap **every materialised item** in an
  `IndexedSemantics` (`addSemanticIndexes`, on by default). FLUI's do not.
- The better design — already identified, and measured — derives each item's
  position from the index the sliver **already stamps** in
  `SliverMultiBoxAdaptorParentData`. Zero extra nodes, and an index the band
  walk keeps in step with the row's real position rather than one captured at
  build time. A working version measured the wrapper cost: a three-item
  `ListView` went from 8 render nodes to 11.
- `IndexedSemantics` / `RenderIndexedSemantics` ship as public widgets for
  content you index yourself. That stays.

So this issue is **not** "port `addSemanticIndexes`". It is: finish the
threading the better design needs.

## Correction: the named blocker is anticipatory, not current

FLUI has **no** `separated` constructor and no separator concept —
`rg -il separator crates/flui-widgets/src crates/flui-objects/src` is empty, and
`ListView` offers only `new`/`builder`. The entry describes a shape FLUI will
have once #546 lands `ListView.separated`, not one it has. Every lazy delegate
shipping today maps logical index 1:1 to set position.

That does **not** justify the shortcut of deriving from the logical index now:
the mapping entry deferred precisely so a *wrong* total is never shipped, and
the shortcut would ship that with a longer fuse. It also could not be corrected
later without this same threading, since the delegate is the only thing that
knows an item is not a set member and the assembler never sees the delegate.

What it changes is sequencing: this is not blocked on `separated`, and should
land before it, so that constructor's author inherits a hook they must answer
rather than a 1:1 assumption they must discover.

## The one blocker, named exactly

`SliverList::separated` interleaves items at even logical indices with
separators at odd ones. A derivation from the logical index alone announces
separators as set members and gives the real items positions 1, 3, 5.

The reference has the same problem and solves it with
`semanticIndexCallback: (Widget, int localIndex) -> int?` — **null meaning "not
a member of the set"**, which is exactly what a separator is.

## Shape

A semantic index travels **beside** the logical one, never derived from it:

1. **`ElementCore::sliver_slot` carries a pair,** not a `usize`. Something like
   `SliverSlot { logical: usize, semantic: Option<i32> }`. A pair by
   construction, because the failure this whole entry is about is exactly the
   two drifting apart. `child_sliver_slot` mints it at the sparse host
   (`unified.rs:209`), passes it through components, and a render element
   consumes it.
2. **Both stamp sites write both.** `SliverMultiBoxAdaptorParentData` gains the
   semantic index (`Option<i32>`); the existing `index: usize` is untouched,
   since layout and the band walk key on the logical one.
3. **The delegate supplies the rule.** Default: semantic == logical.
   `SliverList::separated`: `Some(k)` for logical `2k`, `None` for odd. This is
   `semanticIndexCallback`, and `semanticIndexOffset` is an addend on the same
   value.
4. **The assembler publishes both halves.** The mapping entry is explicit that
   `position_in_set` AND `size_of_set` belong on the **item** node, and that a
   set size on the container is invisible to a reader querying the focused row.
   The size comes from a **semantic** child count — the count of members, not of
   render children — which is why it waits on this same threading and ships
   here, not before.

## The existing test is a tripwire, and must be updated deliberately

`an_indexed_item_publishes_its_position_in_the_set`
(`crates/flui-widgets/tests/semantics.rs`) asserts that **nothing** publishes a
set size, so that the deferral is a checked state rather than an oversight. It
is designed to fail the moment a size appears. Updating it is part of this work,
and the mapping-decision entry it guards must be rewritten in the same change —
not left describing a deferral that has ended.

## Oracles

`'SliverFixedExtentList.builder should respect semanticIndexOffset'` and the
`list_view_test.dart` semantics cases. Assert on the published AccessKit nodes,
not the framework configuration: the mapping entry notes the whole chain
"existed in pieces before this and connected to nothing", so the near end proves
nothing about what a reader receives.

## Slices

1. `SliverSlot` pair through `ElementCore` + both stamp sites; semantic index
   defaults to the logical one. No behaviour change, all existing tests hold.
2. The delegate rule (`semantic_index_callback` + `semantic_index_offset`), with
   `SliverList::separated` supplying `None` for separators.
3. Publish `position_in_set` from the semantic index; retire the wrapper
   requirement for lazy children.
4. Semantic child count → `size_of_set`; update the tripwire test and the
   `## Mapping decisions` entry together.
