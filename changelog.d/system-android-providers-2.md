### Added

- `flui-system` on Android: `Launcher` opens a URI through `ACTION_VIEW` and a file through a
  content URI only the activity that opens it may read; `FileDialogs` picks and saves documents
  through the Storage Access Framework, a pick answered as its name and contents within the
  request's `max_bytes` and a save writing the request's contents; `MessageBoxes` shows an
  `AlertDialog`; `Share` shows the system's share sheet with text, a link and files, each file
  shared under the name it ends at; `UserDirs` names the public Documents, Download, Music,
  Pictures and Movies folders; `Os` says the Android version and the device's architecture.
  Autostart, global shortcuts, the launcher entry, the jump list, reveal and trash answer
  `NotOnThisPlatform`, a folder dialog `NotYetOnThisHost`, and a save without contents
  `NotOnThisPlatform`.
- `[package.metadata.flui.declares]` schema 4 adds `android.providers`: a crate's content provider,
  by its class and an authority suffix under the application's id, which `flui build` writes into
  the generated manifest, not exported and granting per URI (as `${applicationId}.<suffix>` when
  Gradle packs the APK, so a variant's `applicationIdSuffix` reaches it). An application that
  links `flui-system` gets its provider and its Java classes in the APK without declaring anything.
- `flui_platform::capability::Activity::on_main_thread`: a capability provider's work for Android's
  main thread, run by the host inside its panic boundary and answered through a `Completion`, or
  refused with `MainThreadRefused` (`ActivityEnded`, `TooMuchPosted` past `MAX_POSTED_WORK`,
  `HandOffFailed`). `ActivityBridge` gains `post`, which a host implements.

### Changed

- `cargo xtask android-dex` checks and writes the dex of every workspace crate that declares
  `android.dex` and `android.java`, read from `cargo metadata`; `--list` names them, and the
  `Android dex` workflow runs on any `android/java` tree and uploads what it lists.
