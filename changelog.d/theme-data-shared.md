### Changed

- `flui_material::Theme::of` and `Theme::maybe_of` return the ancestor's `Rc<ThemeData>` instead of
  a copy: every Material widget read one on each build, and the copies were 12-27% of a frame in
  the app scenario bench. Fields read through the `Rc` as before; a non-`Copy` field taken by
  value now needs `.clone()` (`theme.text_theme.title_large.clone()`). `Theme::new` takes a
  `ThemeData` or an `Rc<ThemeData>`.
