### Added

- `flui add <capability>` links a capability crate: it enables the facade feature that carries it
  (`flui add system`) or, without the facade, adds the crate from the application's FLUI checkout,
  then prints the line that registers its plugin, such as
  `.plugin(flui::services::system::SystemPlugin::default())`, and what the crate declares for the
  manifests. `--json` emits an `add.done` event.
- A capability crate states its plugin type in `[package.metadata.flui.capability] plugin`;
  `flui-system` declares `SystemPlugin`.
