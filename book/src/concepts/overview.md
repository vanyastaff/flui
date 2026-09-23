# Concepts

FLUI's mental model is Flutter's: declarative widget composition over a retained three-tree.
These pages explain the tree and its rules; if you already know Flutter, read
[Flutter → FLUI mapping](../mapping.md) first for the vocabulary, then come back here for the
parts that differ.

- [View, Element, RenderObject](view-element-render.md) — the three trees themselves.
- [Keys](keys.md) — identity and reparenting.
- [Lifecycle](lifecycle.md) — the states an `Element` moves through.
- [Layout: constraints down, sizes up](layout.md) — the layout protocol.
- [State: setState, InheritedView, ValueNotifier](state.md) — the ways state enters the tree.

Every claim on these pages is checked against the real trait/struct definitions in `crates/` —
see AGENTS.md's Design stance for why that matters more here than it would in most frameworks'
docs: a divergence from Flutter is only valid when it's deliberate and recorded, so this book
cannot afford to describe a contract that doesn't match the code.
