### Removed

- **Breaking:** `flui_view::element::ElementKind` loses its `RenderLeaf`, `RenderSingle` and
  `RenderOptional` variants, which no element ever instantiated. The `Stateful` variant is now a
  tuple variant: it no longer carries an `animation_listener` field.
- **Breaking:** `flui_view::element::AnimationListener` is removed; nothing constructed it.
- **Breaking:** `RenderElementBase` loses its arity type parameter (`RenderElementBase<A>` is now
  `RenderElementBase`), and `ElementArity` is no longer implemented for `Leaf` and `Optional`,
  which remain render-object arities in `flui_foundation`.
