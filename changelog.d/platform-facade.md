### Changed

- `flui::platform` is a module that names what an application uses (system preferences, locale,
  displays, window options, storage, launch, the clipboard's answers) and the host contracts
  other facade signatures name, each at one path. It was `pub use flui_platform::*`, with every
  item of `keyboard`, `pointer`, `text_store` and `data_transfer` at a second path. The keys and
  pointer events widgets use are named at `flui::interaction` (`flui::interaction::NamedKey`,
  not `flui::platform::keyboard::NamedKey`); the host-side rest of an input module stays under
  that module (`flui::platform::pointer::PointerCancel`). A package names the rest from
  `flui_platform`, a contract crate it may depend on (ADR-0088).
- Without the `unstable-input` feature, `flui::platform::haptics` and
  `flui::platform::input_capabilities` do not exist, and the compiler says which feature gates
  them.

### Removed

- `flui::platform::testing`, the host-side test doubles (`ScriptedHttp`, `ScriptedLauncher`,
  `FakeHaptics`); an application's tests use `flui::testing`.
- From `flui::platform`, the items nothing in the facade names: the display registry and window
  id mints a host runs, `WindowHandler`, the launch key, `Backoff`, `EventTime` (named at
  `flui::interaction`).
