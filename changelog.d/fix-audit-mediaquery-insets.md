### Added

- `MediaQueryData::view_padding` (the raw OS safe area) with `set_view_padding`, `set_view_insets` and `remove_bottom_view_inset`, which keep `padding`, `view_padding` and `view_insets` consistent.

### Fixed

- `MediaQueryData::padding` is now the safe area not already covered by `view_insets`, so a `SafeArea` no longer reserves the home-indicator inset above an open keyboard. Code that built `MediaQueryData` with a struct literal and a nonzero `padding` should call `set_view_padding` instead.
