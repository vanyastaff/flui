# Concepts

FLUI's mental model is Flutter's: declarative widget composition over retained trees.
These pages explain the tree and its rules; if you already know Flutter, read
[Flutter → FLUI mapping](../mapping.md) first for the vocabulary, then come back here for the
parts that differ.

- [View, Element, RenderObject](view-element-render.md) — the trees themselves.
- [Keys](keys.md) — identity and reparenting.
- [Lifecycle](lifecycle.md) — the states an `Element` moves through.
- [Layout: constraints down, sizes up](layout.md) — the layout protocol.
- [State: setState, InheritedView, ValueNotifier](state.md) — the ways state enters the tree.

Every claim on these pages is checked against the real trait/struct definitions in `crates/`:
FLUI differs from Flutter in many places by design, so a page written from Flutter's docs rather
than from the code would describe a contract FLUI does not have.
