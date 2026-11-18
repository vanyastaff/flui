### Added

- `flui-system` on Android: `Launcher` opens a URI through `ACTION_VIEW` and a file through a
  content URI only the activity that opens it may read; `FileDialogs` picks and saves documents
  through the Storage Access Framework, a pick answered as its name and contents within the
  request's `max_bytes` and a save writing the request's contents; `MessageBoxes` shows an
  `AlertDialog`; `Share` shows the system's share sheet with text, a link and files; `UserDirs`
  names the public Documents, Download, Music, Pictures and Movies folders; `Os` says the Android
  version and the device's architecture. Autostart, global shortcuts, the launcher entry, the jump
  list, reveal and trash answer `NotOnThisPlatform`, a folder dialog `NotYetOnThisHost`, and a save
  without contents `NotOnThisPlatform`.
- `[package.metadata.flui.declares]` schema 4 adds `android.providers`: a crate's content provider,
  by its class and an authority suffix under the application's id, which `flui build` writes into
  the generated manifest, not exported and granting per URI. An application that links
  `flui-system` gets its provider and its Java classes in the APK without declaring anything.

### Changed

- `cargo xtask android-dex` checks and writes the dex of every crate that ships Java classes
  (`flui-native`, `flui-system`), and the `Android dex` workflow uploads each.
