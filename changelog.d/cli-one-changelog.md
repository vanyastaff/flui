### Added

- **`flui-cli`**: `flui run --device <serial>` for an online Android device or emulator reports
  `run.android.install {serial, apk}` and `run.android.start {package, pid}` under `--json`, and the
  app's `adb logcat --pid` output follows as the session's output (`run.app.log` under `--json`); a
  change rebuilds, reinstalls and restarts, `q`/Ctrl-C force-stop the app. An offline or
  unauthorized device is refused with exit 5 and the reason.
- **`flui-cli`**: `flui doctor --android` checks the SDK `build-tools` (aapt2, zipalign,
  apksigner) and `cargo-ndk`.
- **`flui-cli`**: `flui run --device browser:<name>` for any browser `flui devices` lists: the
  project is built for `wasm32-unknown-unknown`, served by a built-in dev server on `127.0.0.1`
  (`--web-port`, default a free port), the browser is opened (`--no-open` prints the URL instead),
  and every rebuild reloads the page. `--json` adds `run.web.serve {url, dir}`,
  `run.web.open {browser, url}` and `run.reload {kind: "page", ok}`.

### Changed

- **`flui-cli`**: the `counter`, `basic` and `empty` templates put the application in
  `src/lib.rs` with a `#[cfg(target_os = "android")] android_main` beside it, and keep
  `src/main.rs` as the one-line desktop entry. The root view is `pub`. The scaffolded
  `AndroidManifest.xml` declares no icon (no `res/` is needed; the platform's default applies).
- **`flui-cli`**: `flui build web` builds a generated project: the `fn main` binary is compiled
  for wasm32 and `wasm-bindgen --target web` writes `pkg/app.js` and `pkg/app_bg.wasm` beside
  `index.html` in the output directory. The previous pipeline ran `wasm-pack` in a
  `crates/flui_app` directory no template creates. `wasm-bindgen-cli` must match the project's
  `wasm-bindgen` crate; the build checks and prints the exact `cargo install` otherwise.
  `flui doctor` reports `wasm-bindgen`, not `wasm-pack`.

### Fixed

- **`flui-cli`**: `cargo binstall flui-cli` installs the prebuilt Linux and macOS binaries. The
  `[package.metadata.binstall]` of 0.1.0 put both binaries at the root of every archive, but the
  Linux and macOS archives hold them in a `flui-<target>/` folder, so binstall found no binary
  there and fell back to building from source. Windows was unaffected.
