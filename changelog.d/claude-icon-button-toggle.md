### Added

- `IconButton::selected` and `IconButton::selected_icon` make an icon button a toggle: a
  selected one takes the `primary` color and state layer, the node reports `Toggled`, and the
  swap to the selected icon cross-fades through `SymbolEffect::replace`.
