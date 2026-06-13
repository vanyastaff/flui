# AGENTS.md — flui-tree

Generic tree abstraction traits. Every concrete tree (LayerTree, SemanticsTree, RenderTree, ElementTree, ViewTree) implements the same trio.

## Core traits

- `TreeRead<I>` — read-only access (`get`, `contains`, `len`)
- `TreeNav<I>` — navigation (`parent`, `children`, `ancestors`, `descendants`)
- `TreeWrite<I>` — mutations (`insert`, `remove` cascade-by-default, `remove_shallow` opt-out)

## What lives here

- Arity system: `Arity`, `Single`, `Variable`, `Leaf`, `Never`, `Optional`, etc.
- Depth system: `Depth`, `AtomicDepth`, `MAX_TREE_DEPTH`
- Iterators: `Ancestors`, `Descendants`, `Siblings`
- Slot system: `Slot`, `IndexedSlot`, `SlotBuilder`
- Error types: `TreeError`, `TreeResult`

## Key constraints

- Re-exports `ElementId`, `Identifier`, `TreeId` from `flui-foundation`
- The `visitor` and `diff` modules were deleted (zero consumers). Don't re-add speculative scaffolding
- Uses `bon` for typed builders
