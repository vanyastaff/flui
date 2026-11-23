### Changed

- `TextStyle`, `StrutStyle` and `IconData` name their fonts with `FontFamilyName` (a shared name) and `SharedSlice<FontFamilyName>` (a shared list) instead of `String` and `Vec<String>`, so copying a style no longer copies its names. `with_font_family` takes `impl Into<FontFamilyName>`, so `"Roboto"` and a `String` still work; a struct literal writes `Some("Roboto".into())` and `["serif"].into()`.

### Fixed

- Recording a paragraph no longer formats each span's family: the text `describe_spans` prints is made when it is asked for.
