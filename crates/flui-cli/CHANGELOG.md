# Changelog

All notable changes to `flui-cli` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate shares
the workspace version and follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `--json` on every command: one NDJSON object per line on stdout with an
  `event` field, human text on stderr only. `flui run --json` forwards the
  app's stdout and stderr as `run.app.log {stream, line}` and closes every
  session with `run.stop {interrupted}`.
- `-q`/`--quiet`, `-v`/`--verbose`, `--color auto|always|never` (honouring
  `NO_COLOR`, `CLICOLOR_FORCE`, `TERM=dumb`) and `--non-interactive`
  (implied by `CI`, `FLUI_NON_INTERACTIVE`, or a non-terminal stdin).
- An exit-code contract: 0 success, 1 failure, 2 usage or unsupported
  target, 3 environment, 4 build/test/lint/format failure, 5 device not
  found, 6 not a FLUI project, 7 needs a terminal, 130 interrupted.
- `flui run` hot-keys on Unix terminals: `r` reload, `R` restart, `c`
  clear, `h` legend, `q` quit; Ctrl-C always stops the app first and exits
  130. `--device` accepts an id, a name or a unique prefix from
  `flui devices`.
- `flui doctor --fix` installs missing `rustup` targets; `--android`,
  `--ios`, `--web` promote a toolchain's warnings to errors. Every external
  probe has a 10-second deadline.
- `flui create --dry-run` lists exactly the files a real run writes;
  templates `counter`, `basic`, `empty` and `widget` (`--lib`), each
  compile-tested.
- `flui upgrade --check` compares against crates.io with SemVer 2.0
  prerelease precedence; `--check --dependencies` reports what
  `cargo update` would change.
- `flui platform remove --yes` for scripts.
- Shell completions travel inside a `completions` event under `--json`.
- `cargo flui <command>`: a `cargo-flui` binary ships beside `flui` and
  runs it with the same arguments, output and exit code.
- Prebuilt binaries for Linux (x86_64, aarch64), macOS (Intel, Apple
  silicon) and Windows (x86_64) on every `v*` release, with `SHA256SUMS`;
  `cargo binstall flui-cli` installs them.

### Changed

- The CLI is a standalone binary: the build pipeline (once the
  `flui-build` crate) and the source watcher (once `flui-hot-reload`'s
  `source-watch` feature) are modules of this crate, and no FLUI crate is
  in its dependency graph. `cargo install flui-cli` compiles no framework
  code.
- Terminal output is drawn with `console`; prompts run on `dialoguer`.
  `cliclack`, `pollster`, `tracing`, `tracing-subscriber` and `flui-log`
  are no longer dependencies. `RUST_LOG` is not read; `-v` prints
  diagnostics (the commands run, the probes made, the paths skipped) as
  dimmed `debug:` lines on stderr.
- Error messages follow the `thiserror` convention: lowercase, no trailing
  period, and the cause chain printed once under `Caused by:`.
- Platform build tools (Gradle, `wasm-pack`, `xcodebuild`, `cargo ndk`,
  `adb`) are killed when the build that started them is cancelled, and
  `flui build android` passes `JAVA_HOME` to the Gradle wrapper. Without
  `JAVA_HOME` the native libraries are still built and the APK step is
  skipped, which is what the warning always said.
- `clap` is pulled with `derive` only: the `cargo` and `env` features
  enabled macros and attributes no code used. `serde` is a local
  dependency with `derive` alone rather than the workspace's `rc` set.
  `cargo shear` and `cargo outdated` report nothing for this crate.
- iOS simulator probes (`simctl`, `plutil`) run through the same bounded
  process runner as every other probe instead of spinning up an async
  runtime per call.
- `flui.toml` models exactly the documented keys (`[app]`, `[build]
  target_platforms`, `[hot_reload]`); `flui platform add/remove` no longer
  writes unread `[assets]`, `fonts`, `lto` or `opt_level` keys back into the
  file. Unknown keys from older files are ignored.
- `flui create --hot-reload` generates a host / worker / types workspace.
  `flui build ios --lib --universal` produces an XCFramework;
  `flui build macos` stages a launchable `.app`.

### Fixed

- `flui devices` hung on macOS: browser versions are now read from the
  installed bundle's `Info.plist` instead of launching Safari.
- `flui doctor` treated the macOS Java stub as an installed JDK, and could
  exit 0 after reporting failed checks.
- `flui upgrade --self` installed a package named `flui_cli`; the crate is
  `flui-cli`.
- `flui create`'s `git init` ran in the caller's directory rather than the
  new project's.
- The crate did not compile on Windows (the Ctrl-C listener asked tokio
  for an I/O driver its `signal` feature does not expose there).
- Ctrl-C during `flui run` could leave the app running or report the exit
  as a run failure instead of 130.

### Removed

- The `todo`, `dashboard` and `plugin` templates, `flui devtools`,
  `build --split-per-abi`, `build --optimize-wasm` and `test --platform`:
  each was a placeholder with no implementation behind it.
- Global (per-user) configuration and telemetry settings: the CLI stores
  nothing about you and sends nothing anywhere.
