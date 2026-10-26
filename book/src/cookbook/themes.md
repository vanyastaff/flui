# Themes

An application wraps its root view in a theme, then calls `run_app`: `Theme::new(ThemeData::light(), ...)`
for Material, `CupertinoTheme` for Cupertino. An application that also wants light/dark switching,
a navigator and a `ScaffoldMessenger` roots itself in `MaterialApp`
(`packages/flui-material/src/app.rs`) instead, which resolves `ThemeMode` and publishes the
`ThemeData` through `Theme` itself; see the [Flutter → FLUI mapping](../mapping.md) notes.
