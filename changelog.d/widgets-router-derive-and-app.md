### Added

- **`#[derive(Routable)]`** (ADR-0093, `flui-macros`, re-exported beside the trait by
  `flui_widgets`, its prelude and `flui::prelude`): one `#[route("/note/:id")]` pattern per
  unit or named-field variant generates `to_path` and `from_path`. Fields print through
  `Display` and parse through `FromStr`; a `:name` segment fills the field `name`. Malformed
  patterns, parameters without fields, fields without parameters, tuple variants, generic
  types and two patterns of the same shape are compile errors. Parsing goes by specificity, so
  a literal segment wins over a parameter in the same place whatever the declaration order.
- **`WidgetsApp::router(Router<R>)`** (`flui-widgets`): roots an app in a typed `Router`,
  which becomes the app's routing subtree and its only navigator, below `Localizations` and
  the `builder` hook. It returns a `WidgetsApp<RouterForm>`, which has no `navigator` or
  `observer` builder; `WidgetsApp` alone still names the navigator form
  (`WidgetsApp<NavigatorForm>`), and the sealed `AppForm` trait bounds the two.
- **`two_screens` example**: a derived route enum, `WidgetsApp::router`, and pages that push
  and pop through the `RouterHandle` they take in `init_state`
  (`cargo run --example two_screens`).
