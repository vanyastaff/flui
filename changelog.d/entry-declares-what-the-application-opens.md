### Added

- `#[flui::main]` reads the URL schemes and web domains the crate's `[package.metadata.flui.declares]`
  lists (`url-schemes`, `associated-domains`, a feature's while it is on), and the run its function
  starts opens those links without `Application::declares`; declarations given to
  `Application::declares` replace them.

### Changed

- `flui build` refuses `url-schemes` and `associated-domains` in a crate another crate of the
  build depends on: the application declares the links it opens.
