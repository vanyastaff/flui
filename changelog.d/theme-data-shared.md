### Changed

- `flui_material::Theme::of` and `Theme::maybe_of` return the ancestor's `Rc<ThemeData>` instead of
  a copy: a `ThemeData` is over 14 KB, and every Material widget read one on each build. Fields
  read through the `Rc` as before; a non-`Copy` field taken by value now needs `.clone()`
  (`theme.text_theme.title_large.clone()`). `Theme::new` takes a `ThemeData` or an
  `Rc<ThemeData>`.
