# flui

[![crates.io](https://img.shields.io/crates/v/flui-cli.svg)](https://crates.io/crates/flui-cli)
[![CI](https://github.com/vanyastaff/flui/actions/workflows/ci.yml/badge.svg)](https://github.com/vanyastaff/flui/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.97-blue.svg)](Cargo.toml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)

The command-line tool for the [FLUI](https://github.com/vanyastaff/flui)
framework. It scaffolds projects, runs them with hot reload, builds for every
platform, and checks your environment.

- **Fast to install and start.** A standalone binary with no FLUI crate in its
  dependency graph: `cargo install flui-cli` compiles no framework code, and
  the tool starts in about 25 ms.
- **Scriptable.** Every command has `--json` (NDJSON on stdout), a documented
  exit-code contract, and `--non-interactive`; human text never touches
  stdout.
- **Honest.** No telemetry, no network access unless a command asks for it,
  no placeholder features: everything listed here works and is tested.
- **Familiar.** The commands and hot-keys follow `flutter`; see
  [Coming from Flutter](#coming-from-flutter).

## Installation

```bash
cargo install flui-cli --locked
```

From a checkout (the CLI and the framework then share one source tree):

```bash
git clone https://github.com/vanyastaff/flui.git
cd flui
cargo install --path crates/flui-cli --locked
```

Requires Rust 1.97 or newer with Cargo, rustup and Git. Platform toolchains
are only needed for the platforms you build: Xcode command line tools for
macOS and iOS; the Android SDK (`ANDROID_HOME`), NDK, a JDK and `adb` for
Android; the `wasm32-unknown-unknown` target and `wasm-bindgen` or
`wasm-pack` for the web. `flui doctor` tells you what is missing and how to
fix it.

## Quick start

```bash
flui doctor                      # is the machine ready?
flui create my_app               # counter template, git init, cargo check
cd my_app
flui run                         # build, launch, watch; r / R / q keys
flui build macos --release       # a signed-ready .app, path printed at the end
```

Inside the FLUI checkout, generate against the local source instead of the
registry: `flui create my_app --local --path ../apps`.

## Commands

| Command | What it does |
|---------|--------------|
| `flui create [NAME]` | Scaffold a project from a template. Prompts when `NAME` is omitted and a terminal is attached. |
| `flui run` | Build and run with hot reload; hot-keys while running. |
| `flui build <platform>` | Build for `desktop`, `macos`, `windows`, `linux`, `ios`, `android` or `web`; prints every artifact with its size. |
| `flui test` | `cargo test` with `--unit`, `--integration`, `--release`, and harness args after `--`. |
| `flui analyze` | `cargo clippy --workspace --all-targets -D warnings` (`--pedantic`, `--fix`). |
| `flui format` | `cargo fmt --all` (`--check` exits 4 when unformatted). |
| `flui clean` | Cargo artifacts, plus platform build dirs with `--deep` or `--platform`. |
| `flui doctor` | Environment checks with fix hints; `--fix` installs missing `rustup` targets. |
| `flui devices` | Desktop, Android (`adb`), iOS simulators and browsers, with the ids `--device` takes. |
| `flui emulators list|launch` | List and start Android AVDs and iOS simulators. |
| `flui platform add|remove|list` | Manage the platforms declared in `flui.toml` (`remove --yes` for scripts). |
| `flui upgrade` | Update project dependencies; `--self` reinstalls the CLI; `--check` only reports. |
| `flui completions <shell>` | Completion script for bash, zsh, fish, PowerShell or elvish. |

Every command accepts the global flags:

| Flag | Effect |
|------|--------|
| `--json` | One JSON object per line on stdout; nothing decorative anywhere. |
| `-q`, `--quiet` | No progress narration; warnings, errors and tool output stay. |
| `-v`, `--verbose` | Diagnostics on stderr: the commands flui runs, the probes it makes, the paths it skips. |
| `--color auto|always|never` | `auto` honours `NO_COLOR`, `CLICOLOR_FORCE` and `TERM=dumb`. |
| `--non-interactive` | Never prompt or read keys; implied by `CI`, `FLUI_NON_INTERACTIVE`, or a non-terminal stdin. |

Human output always goes to stderr. Stdout carries only payloads you would
pipe: completion scripts and `--json` events.

## Creating projects

```bash
flui create my_app                          # counter (default)
flui create my_app --template basic         # Hello, FLUI! with a Material theme
flui create my_app --template empty         # smallest runnable app
flui create my_widgets --lib                # widget library with a widget test
flui create my_app --hot-reload             # host / worker / types workspace
flui create my_app --org com.example --platforms macos,ios,web
flui create my_app --dry-run                # list the files, write nothing
flui create my_app --no-check               # skip the post-scaffold cargo check
```

Every template listed by `--template` generates a distinct, compile-tested
project; there are no placeholders. Generated files are deterministic (no
timestamps, no random ids), and `flui` is the only framework dependency
(`use flui::prelude::*;`).

`--local` points the dependency at a FLUI checkout: bare `--local` means the
current directory, `--local=/path/to/flui` an explicit one. The `=` is
required so that `flui create --local my_app` still treats `my_app` as the
project name. `--path` chooses the parent directory of the new project.

Without a name and with a terminal attached, `flui create` runs a wizard. In
CI (or with `--non-interactive`) it exits 7 with the flags to pass instead of
hanging.

## Running

```bash
flui run                                    # debug, hot reload
flui run --release                          # optimized, no hot reload
flui run --no-hot-reload                    # build and run once
flui run --device 90D572B1-...              # an iOS simulator UDID from `flui devices`
flui run --profile bench
```

While the app runs and stdin is a terminal:

| Key | Action |
|-----|--------|
| `r` | Rebuild and reload (worker hot reload keeps state in a `--hot-reload` project) |
| `R` | Stop, rebuild everything, start again |
| `c` | Clear the screen |
| `h` | Show the key legend |
| `q` or Ctrl-C | Stop the app and exit |

Hot reload works in two modes, chosen automatically:

- **Process restart**: `src/` and `Cargo.toml` are watched; on change the app
  is stopped, rebuilt and restarted.
- **Worker host** (projects created with `--hot-reload`): the host keeps
  running and only the worker `cdylib` is rebuilt, so widget state survives a
  reload. A change to the shared types crate triggers a host restart.

Ctrl-C always stops the app before the CLI exits and reports exit code 130;
`q` stops it and exits 0, so supervisors can tell the two apart. Hot-keys need
a Unix terminal; elsewhere the loop still runs, driven by file changes and
Ctrl-C.

Android scene hot reload (rebuild and push a scene plugin without restarting
the app): `flui run --scene --scene-crate <crate> --package <android.package>`.

## Building

```bash
flui build desktop --release                # this machine's binary
flui build macos --release                  # staged .app bundle
flui build macos --universal                # arm64 + x86_64 binary fused with lipo (not bundled)
flui build ios --simulator <UDID>           # native app for one simulator
flui build ios --lib --universal            # XCFramework instead of an app
flui build android --release
flui build web --release
flui build desktop --example widgets_gallery   # inside the FLUI checkout
flui build desktop --package my-app-host
flui build desktop --output dist/
```

Selector conflicts (`--lib` with `--example`, `--simulator` on a non-iOS
target, `--universal` without `--lib` on iOS) are rejected before any build
starts. Every build ends with the artifact list and `Built in N.Ns`.

## Checking the environment

```bash
flui doctor
flui doctor --android --fix    # missing Android rustup targets are installed
flui doctor --json
```

Required checks (Rust at or above the CLI's MSRV, host target, Cargo, rustup,
Git) make the command exit 3 when they fail. Android, iOS and Web toolchains
only warn unless you select them explicitly with `--android`, `--ios` or
`--web`. Every external probe has a 10-second deadline; a hung tool is
reported, never waited on. Java's macOS stub (which prints "Unable to locate a
Java Runtime") is detected as missing, not as installed.

## Devices and emulators

```bash
flui devices
flui devices --platform ios --details
flui emulators list
flui emulators launch "iPhone 17 Pro"       # exact name, id, or unique prefix
```

Discovery never fails the command: a missing `adb` or Xcode is reported as a
problem row with a hint. On macOS and Windows browser versions are read from
the installed bundle, never by launching the browser; on Linux a bounded
`--version` probe is used.

`flui run --device` accepts any id or name from this list, or a unique
prefix. Today it can drive this machine and iOS simulators; an Android
device or a browser is refused with exit code 2 and the command to use
instead.

## Machine-readable output

`--json` turns every command into an NDJSON stream on stdout. Each line is an
object with an `event` field; human text, if any, goes to stderr.

| Event | Emitted by |
|-------|------------|
| `doctor.check`, `doctor.fix`, `doctor.summary` | `doctor` |
| `device`, `devices.summary` | `devices` |
| `emulator`, `emulators.summary`, `emulator.launch` | `emulators` |
| `create.start`, `create.file`, `create.done` | `create` (also with `--dry-run`) |
| `run.start`, `run.build.start`, `run.build.done`, `run.app.start`, `run.app.log`, `run.app.exit`, `run.change`, `run.reload`, `run.app.stop`, `run.stop` | `run` |
| `build.start`, `build.phase`, `build.done` | `build` |
| `test.start`, `test.done`, `analyze.done`, `format.done` | `test`, `analyze`, `format` |
| `clean.removed`, `clean.done` | `clean` |
| `upgrade.check`, `upgrade.check_dependencies` | `upgrade --check` (crates.io) / `--check --dependencies` (`cargo update --dry-run`) |
| `platform.added`, `platform.removed`, `platform.list` | `platform` |
| `completions` | `completions` (the script travels inside the event) |
| `error` | any command that fails (`message`, `code`) |

In `--json` mode `flui run` pipes both of the app's output streams and
forwards each line as `run.app.log {stream: "stdout"|"stderr", line}`, so a
tool driving the dev loop sees the build, the app and everything it prints
on one stream. `run.stop {interrupted}` closes every `run.start`; a failure
or Ctrl-C then adds the final `error {code}` event.

```bash
flui devices --json | jq -r 'select(.event=="device" and .platform=="ios") | .id'
```

## Exit codes

| Code | Meaning |
|-----:|---------|
| 0 | Success (also: a prompt cancelled on purpose) |
| 1 | Generic failure |
| 2 | Usage error (unknown flag, conflicting selectors) or an unsupported target |
| 3 | Environment: a required tool is missing or `doctor` found errors |
| 4 | The project's build, tests, lints or format check failed |
| 5 | The requested device or emulator does not exist |
| 6 | Not a FLUI project (wrong directory, or a library crate given to `run`) |
| 7 | A prompt was needed but the session is non-interactive |
| 130 | Interrupted with Ctrl-C |

## Configuration (`flui.toml`)

```toml
[app]
name = "my_app"
version = "0.1.0"
organization = "com.example"

[build]
target_platforms = ["macos", "ios", "web"]

# Written by `flui create --hot-reload`; makes `flui run` use the worker host.
[hot_reload]
host_package = "my_app-host"
worker_package = "my_app-logic"
worker_lib = "my_app_logic"
logic_watch = "logic/src"
types_watch = "types/src"
```

`[app].name` and `[app].organization` become the macOS bundle name and
identifier. Unknown keys are ignored. There is no global configuration file
and no telemetry setting, because the CLI stores nothing about you and sends
nothing anywhere.

## Coming from Flutter

| Flutter | FLUI |
|---------|------|
| `flutter create app` | `flui create app` |
| `flutter run` + `r` / `R` / `q` | `flui run` + `r` / `R` / `q` |
| `flutter devices --machine` | `flui devices --json` |
| `flutter emulators --launch x` | `flui emulators launch x` |
| `flutter doctor -v` | `flui doctor -v` (plus `--json` and `--fix`) |
| `flutter build macos` / `apk` / `web` | `flui build macos` / `android` / `web` |
| `flutter analyze` / `flutter test` | `flui analyze` / `flui test` |
| `flutter --suppress-analytics` | not needed; there are none |
| `flutter logs` | app stdout in the terminal, or `run.app.log` events with `--json` |

## Shell completions

```bash
flui completions bash > ~/.local/share/bash-completion/completions/flui
flui completions zsh  > ~/.zfunc/_flui
flui completions fish > ~/.config/fish/completions/flui.fish
flui completions powershell >> $PROFILE
```

The script is the only thing written to stdout; installation notes go to
stderr and are silenced by `--quiet`. With `--json` the script is delivered
inside a `completions` event instead.

## Contributing

Bug reports and pull requests are welcome at
[github.com/vanyastaff/flui](https://github.com/vanyastaff/flui/issues).
The CLI lives in `crates/flui-cli`; before opening a pull request, run:

```bash
cargo fmt -p flui-cli
cargo clippy -p flui-cli --all-targets -- -D warnings
cargo nextest run -p flui-cli
```

The integration tests drive the built `flui` binary, so every command has
both a human-mode and a `--json` assertion; a change to output or exit codes
belongs in the tables above and in [CHANGELOG.md](CHANGELOG.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE](LICENSE) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
