### Changed

- `TextDirection` now lives in `flui_foundation::text_direction` and is re-exported from
  `flui_painting` and `flui_semantics`, so the two previously-distinct enums are one value and the
  paint, semantics and reading-order paths no longer map between them by hand.

### Removed

- `flui_painting::TextSelection` (`{ base, extent: TextPosition }`), which had no user outside its
  own crate. The text selection a semantics action carries (`flui_semantics::TextSelection`,
  `{ anchor, focus }`, in UTF-8 bytes) is unchanged.
