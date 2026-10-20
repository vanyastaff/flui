### Added

- `MediaQueryData::view_padding` (the raw OS safe area) and `MediaQueryData::padding()`, which is computed as `view_padding` minus `view_insets` per edge, clamped at zero. `remove_padding`, `remove_top_padding`, `remove_bottom_padding`, `remove_bottom_view_inset` and `set_padding_bottom` adjust the sources consistently for widgets that consume or report an inset.

### Changed

- `MediaQueryData::padding` is no longer a field; read `padding()` and set `view_padding` instead. `SafeArea` and the scaffolds honour `view_insets` once a platform publishes them, so they no longer reserve the home-indicator area above an open keyboard. Nothing in the runtime publishes keyboard `view_insets` yet.
