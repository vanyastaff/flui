# Concepts

FLUI's mental model is declarative widget composition over a retained three-tree.
These pages explain the tree and its rules; if you already know Flutter, the
[Flutter → FLUI mapping](../mapping.md) lists the vocabulary.

- [View, Element, RenderObject](view-element-render.md) — the three trees themselves.
- [Keys](keys.md) — identity and reparenting.
- [Lifecycle](lifecycle.md) — the states an `Element` moves through.
- [Layout: constraints down, sizes up](layout.md) — the layout protocol.
- [State: setState, InheritedView, ValueNotifier](state.md) — the ways state enters the tree.

Every claim on these pages is checked against the real trait/struct definitions in `crates/`.
