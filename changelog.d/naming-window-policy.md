### Changed

- `flui_app::WindowPolicy::SeparateRealms` is now `WindowPolicy::Isolated`
  (also `flui::WindowPolicy::Isolated`): the new window gets its own state,
  `GlobalKey` scope and scheduler.
- `flui_app::WindowPolicy::SharedRealm` is now `WindowPolicy::Shared`: a
  second window of the same session.
- `flui_testing::HeadlessRealm` is now `flui_testing::HeadlessHost`.
- The module `flui_testing::realm` is now `flui_testing::host`; its
  `HeadlessWindow`, `HeadlessSink` and `HeadlessDevAgent` keep their names and
  their crate-root re-exports.
- rustdoc search still finds the old names through `#[doc(alias)]`.
