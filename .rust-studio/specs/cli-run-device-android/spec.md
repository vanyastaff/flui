# `flui run --device <android serial>`

Branch `cli/run-device-android` (worktree `flui-cli-wt`, from `main` after #1229).

## Problem

`flui run --device emulator-5554` exits 2. Underneath, a generated project
cannot become an APK at all: the templates are `fn main` binaries with no
`android_main`, `build/android.rs` runs `cargo ndk` on a `crates/flui_app`
manifest no template has, and `flui platform add android` scaffolds Gradle
files without a wrapper, so the APK step always skips itself.

## Design

### Templates (counter, basic, empty)

`src/lib.rs` holds the app and both entry points' targets; `src/main.rs`
is `fn main() { flui::run_app(<crate>::App) }`. `[lib] crate-type =
["rlib", "cdylib"]`: the rlib feeds the binary and the tests, the cdylib is
what `cargo ndk` builds. The Android entry lives in `lib.rs`:

```rust
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: flui::android_activity::AndroidApp) {
    flui::run_app_android(app, App);
}
```

The facade grows `#[cfg(target_os = "android")] pub use flui_app::
{run_app_android, run_app_android_with_config, android_activity}`;
`flui-app` re-exports `android_activity` under the same cfg. The widget
template (`--lib`) and the hot-reload workspace are unchanged; the host of
a hot-reload project targets the desktop.

### Manifest

`android:icon` is dropped: with no resources the platform's default icon
applies and the APK needs no `res/`. `hasCode="false"`, `NativeActivity`,
`android.app.lib_name` = the crate's lib name (`{{lib_name}}`).

### Build (`build/android.rs`)

`build_rust`: `cargo ndk -t <abi> -o <jniLibs> build --lib [--release]` in
the project root (its own manifest). `build_platform`: when
`platforms/android/gradlew` exists, the existing Gradle path; otherwise the
built-in packager, `build/android_package.rs`:

1. `aapt2 link --manifest AndroidManifest.xml -I <sdk>/platforms/android-<N>/android.jar --min-sdk-version 21 --target-sdk-version <N> -o base.apk` (highest installed `build-tools` and `platforms`).
2. Append `lib/<abi>/lib<name>.so` STORED (uncompressed) with the `zip` crate (`new_append`, no compression features needed).
3. `zipalign -f -p 4 base.apk aligned.apk`.
4. `apksigner sign --ks ~/.android/debug.keystore --ks-pass pass:android --ks-key-alias androiddebugkey --key-pass pass:android --out <name>-debug.apk aligned.apk`; the keystore is created with `keytool` when missing. `JAVA_HOME` is resolved as today (env), else `java` on `PATH`.

Output: `<output_dir>/<name>-<profile>.apk`, reported by `flui build android`.

### Run (`commands/run.rs`)

`Target::Android { serial }` for `DevicePlatform::Android` devices that are
`Online` (`Unauthorized`/`Offline` → `DeviceNotFound` with the state in the
hint). `AndroidSession: ReloadStrategy`:

- `build_and_spawn`: build → `adb -s S install -r <apk>` → `adb shell am start -W -n <pkg>/android.app.NativeActivity` → `adb shell pidof <pkg>` (retried for 5 s) → child = `adb -s S logcat --pid <pid> -v brief` (its stdout is the app's log, forwarded like any child). `logcat --pid` exits when the process dies, so `ChildExited` means the app died.
- `on_change`: stop logcat, `am force-stop <pkg>`, rebuild, reinstall, start again. `reload_label`: "Rebuild, reinstall and restart".
- Quit / Ctrl-C: `am force-stop`, logcat stopped by the loop's cleanup.
- Package name: `flui.toml` `[app]` (`AppConfig::app_id()`), the same value `flui platform add android` wrote into `applicationId`.

Events: `run.android.install {serial, apk}`, `run.android.start {package, pid}`; the existing `run.app.*` carry logcat lines.

### Doctor

`--android` already checks SDK/NDK/cargo-ndk; add `build-tools` (aapt2, zipalign, apksigner) and a JDK (`java`), both required for the packager.

## Tests

- Unit: packager argument construction and keystore command; APK entry
  names per ABI; Android target mapping incl. offline/unauthorized devices;
  `am start` component string.
- Template compile tests already cover the lib/main split on the host.
- Live (`FLUI_CLI_LIVE_ANDROID=1`): `flui build android --json` on a
  scaffolded counter yields an APK whose zip contains
  `lib/arm64-v8a/lib<name>.so` and `AndroidManifest.xml`; with a booted
  emulator, `flui run --device <serial> --no-hot-reload --json` emits
  `run.android.start` and the first `run.app.log` line, then exits 130 on
  SIGINT.

## Out of scope

Android for the hot-reload workspace host; Gradle wrapper generation;
`flui run --scene` changes; iOS.
