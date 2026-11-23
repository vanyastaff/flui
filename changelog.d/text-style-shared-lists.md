### Changed

- `TextStyle::font_features`, `font_variations` and `shadows`, and `IconThemeData::shadows`, are
  `SharedSlice` lists, as `font_family_fallback` is: copying a style or an icon's style shares
  them instead of allocating a copy. A struct literal writes `[...].into()` or `vec.into()`;
  the `with_*` builders are unchanged. `FontFamilyFallback` is now `SharedSlice<FontFamilyName>`.
