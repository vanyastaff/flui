### Added

- **System services capability** (`flui-platform`, `flui-system`, `flui`): `flui_platform::launch`
  opens a URI or a file, reveals a file and moves one to the trash through the `Launcher` handle;
  `ExternalUri` refuses relative URIs and the `javascript`, `vbscript`, `data`, `about`, `blob` and
  `file` schemes. The new `flui-system` crate (facade feature `system`, off by default, as
  `flui::services::system`) adds open and save dialogs modal to the widget's window, one at a time,
  and `SystemPlugin`. No host provides them yet: both answer `Unsupported(NotYetOnThisHost)`. Their
  `testing` kits script them (`ScriptedLauncher`, `ScriptedFileDialogs`, `ScriptedSystem`).
- **Capability crates** (`xtask`): `tier-kind = "capability"` is checked by `cargo xtask workspace`
  (tier `pkg`, named only by the facade, normal dependencies on the contract crates and the SDK)
  and by `cargo xtask reach` (`[reach.kind.capability]`).
