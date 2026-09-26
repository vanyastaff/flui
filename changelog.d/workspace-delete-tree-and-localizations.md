### Changed

- **Crate deletions ([ADR-0081](/docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md)): where the
  items went.**
  - `flui::localizations::{GlobalWidgetsLocalizations, GlobalWidgetsLocalizationsDelegate,
    RTL_LANGUAGES}` → `flui::widgets::…` (no feature needed).
  - `flui_localizations::BoxedLocalizationsDelegate` → `flui_widgets::BoxedLocalizationsDelegate`
    (unchanged; the old crate re-exported it).
  - `flui_tree::{Arity, Leaf, Optional, Single, Exact, AtLeast, Range, Variable, Never}` →
    `flui_foundation::…` (`flui::rendering::*` is unchanged).
  - `flui_tree::IndexedSlot` → `flui_foundation::IndexedSlot` (`flui_view::IndexedSlot` is
    unchanged).
  - `use flui_tree::TreeWrite; tree.remove(id)` on a `SemanticsTree` → `tree.remove(id)`:
    `SemanticsTree::remove` is inherent.
  - `TreeWrite::insert` on a `RenderTree` → `RenderTree::insert` (inherent).
  - `flui_semantics::prelude` no longer re-exports `TreeNav`/`TreeRead`.
  - The `TreeNav` walks (`ancestors`, `descendants`, `siblings`, `child_count`, `has_children`,
    `lowest_common_ancestor`, …) are no longer public on `LayerTree`, `RenderTree` or
    `SemanticsTree`, and have no public replacement. Each tree keeps its public `get`, `parent`,
    `children`, `contains`, `len` and `iter` (`RenderTree` also `depth`), which a caller can walk;
    `LayerTree`'s `ancestors`/`lowest_common_ancestor` and `SemanticsTree`'s ancestry check are
    crate-private.
  - The facade's `localizations` feature is empty and deprecated; it is removed once nothing
    names it.

### Removed

- **`flui-localizations`** ([ADR-0081](/docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md)). It
  held no translated strings; its RTL table and delegate moved to `flui_widgets::localization`
  (see Changed for the paths).
- **`flui-tree`** ([ADR-0081](/docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md)). The
  `TreeRead`/`TreeNav`/`TreeWrite` traits had no generic consumer and became inherent methods on
  the trees that used them; the arity markers and `IndexedSlot` moved to `flui-foundation`.
  `Depth`, `AtomicDepth`, `DepthAware`, `Slot`, `SlotBuilder`, `SlotIter`, `TreeError`,
  `ArityError` and the tree iterators had no user and are gone.
