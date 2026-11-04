### Changed

- **`MediaQuery` and `MediaQueryData` live in `flui_view::environment`.**
  The inherited presentation-data types moved out of `flui_widgets` and the
  `flui-widgets::media_query` module is deleted: `flui-widgets` imports the
  types from `flui_view`, and package authors reach them through the SDK's
  `flui_sdk::view::environment` re-export. `flui_widgets::MediaQuery`,
  `flui_widgets::MediaQueryData` and their `flui_widgets::prelude` names are
  gone; the `flui_view` prelude and the facade's `flui::view::environment`
  carry the new paths, so application code on prelude glob imports needs no
  change.
