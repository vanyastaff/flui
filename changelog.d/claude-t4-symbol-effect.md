### Added

- `SymbolEffect` moves an `Icon` or `AnimatedIcon`: one-shot `bounce`, `wiggle` and `rotate`
  played on a changed value, repeating `pulse`, `breathe` and `rotate` while active, and the
  `appear`, `disappear` and `replace` transitions; `repeat`, `speed(Speed)` and
  `animate_on_appear` tune it. Under reduced motion icons stay still or land on a transition's
  end. A replace keeps only the new icon's label in the semantics tree.
- `PathEffect::draw_on`/`draw_off` draws an `AnimatedIcon`'s paths on or off; `Icon` has no way
  to take one.
- `IconThemeData::symbol_effect` applies an effect to every icon below that has none of its
  own. `NavigationBar` uses it to bounce a newly selected destination's icon.
- `FadeTransition::always_include_semantics` keeps a fully transparent child in the semantics
  tree, backed by `RenderAnimatedOpacity::set_always_include_semantics`.

### Changed

- `IconThemeData` is `#[non_exhaustive]`: build it from `IconThemeData::default()` with one
  method per field (`.size(24.0).color(..)`) instead of a struct literal.

### Fixed

- `IndexedStack` keeps its hidden children out of the semantics tree, as its docs said, and
  republishes semantics when its index changes.
