# ADR-0326: One entry starts an application on every platform

- **Status:** Implemented (2026-11-12)
- **Date:** 2026-11-12
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) (the capability registrations a run
  carries), [ADR-0039](ADR-0039-event-loop-affinity-capability.md) (the resident main window),
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §8 (the Activity enters the core host
  through `android_platform`), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md)
  (how a run ends), [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (upstream types in
  Stable signatures)

## Context and problem statement

The entry point is copied into every project, so it is the most expensive public shape to change
after publication. Before this record an application started three ways:

| Fact | Where |
|---|---|
| `run_app(root, AppLaunch)` returned `Result` on the desktop but panicked on an event-loop failure, and panicked on Android, telling the author to call `run_app_android` | `crates/flui-app/src/app/runner/{mod,desktop}.rs` |
| Android needed its own `android_main` calling `run_app_android(app, root, launch)`, written by hand in every project | `crates/flui-cli/src/templates/*.rs` |
| A third, desktop-only builder, `Application::new(factory).run()`, returned another error type, `AppRunError`, beside `LaunchError` | `crates/flui-app/src/app/application.rs` |
| The facade's prelude differed per platform, and `flui::android_activity` was a facade item | `src/lib.rs` |

What does an application write to start, and how does that one form reach Android, whose system
loads the application as a library and calls an exported function?

## Decision drivers

- Flutter writes `void main() => runApp(MyApp())` once for every platform. FLUI departs from that
  only for a Rust reason.
- A public entry never panics on a failure the platform reports: the failure is a typed error.
- One way to do a thing (AGENTS.md, "Current forms"): a configured run and a resident run are the
  same builder, not two types with two error enums.
- A one-file desktop application keeps working with `cargo run` and nothing but `fn main`.
- No new process-global state (ADR-0097).

## Considered options

1. `run_app(root)` and one `Application` builder; an attribute, `#[flui::main]`, writes Android's
   `android_main` beside the application's entry function.
2. A function-like macro, `flui::main!(App)`, that writes `fn main` and `android_main` both.
3. Keep a per-platform entry (`run_app_android`) and let `flui create` write it.

## Decision

Chosen option: 1, because it keeps the author's `fn main` and `run_app(root)` exactly as on every
other target and confines the one Rust-specific need, an exported `android_main` in a `cdylib`, to
an attribute that changes nothing elsewhere.

### §1. `run_app(root)` and one builder

`flui_app::run_app<V: View + Clone>(root: V) -> Result<(), LaunchError>` is
`Application::new().run(root)`. `Application` (non-generic, every platform) carries the
`AppConfig` (`with_config`) and the capability registrations (`capability::<C>`, `plugin`). Its
terminal methods are `run(root)` on every platform and, on the desktop, `run_resident(factory)`,
which starts with no window and builds each main window an `AppHandle` later shows from
`FnMut(&AppHandle) -> V`. The desktop adds `on_ready` and `on_window_error`. `AppLaunch`,
`AppCapabilities` (now crate-private), `AppRunError`, `StartupWindow` and `run_app_android` are
deleted.

### §2. One error

`LaunchError` (`#[non_exhaustive]`) is what every run returns: `CapabilityConflict`,
`UnsupportedSystem`, `Platform { source }` (startup or event-loop failure), on the desktop
`InitialWindow(AppWindowError)`, `OnReadyPanicked`, `Service { source }`, and on Android
`NoAndroidEntry`. A startup window that no GPU backend could give a device is
`UnsupportedSystem::NoSupportedGpu`. Nothing on the start path panics with its own message; an
application's own panic still resumes after the teardown (ADR-0258).

### §3. `#[flui::main]`

`flui_macros::main`, re-exported as `flui::main` and `flui_app::main`, marks the function an
application starts from: a plain `fn` with no arguments returning `()` or `Result<(), E: Display>`.
The attribute leaves the function as it is on every target and adds, under
`#[cfg(target_os = "android")]`, `#[unsafe(no_mangle)] fn android_main(app: AndroidApp)` calling
`flui_app::__entry::android(app, entry)`. That hands the `AndroidApp` to the run through the
`AppRuntime` of the thread (`AppRuntime::android_entry`, inside the existing `APP_RUNTIME`
trampoline, so no new global), calls the entry, and logs its `Err`. `Application::run` on Android
takes it from there; a run that finds none answers `LaunchError::NoAndroidEntry`. The attribute
checks its own arguments and the function's signature, and `__entry::check` checks the return type
on every target, so a function Android could not call fails on the desktop too. `__entry` is
`#[doc(hidden)]`: it is what the attribute expands to, not application surface, and it is the only
place `android_activity` appears in `flui-app`'s public items.

A project generated by `flui create` has `#[flui::main] pub fn run()` in `src/lib.rs` and
`fn main() -> Result<(), flui::LaunchError> { app::run() }` in `src/main.rs`, which the desktop,
iOS and the web (`wasm-bindgen` runs the binary's `main`) start from.

### Consequences

- Good, because the code an author writes to start is the same on every platform, and Android
  needs none of its own.
- Good, because every start failure is a value of one type.
- Bad, because a generated project keeps two files, `lib.rs` for the `cdylib` and `main.rs` for the
  binary: Cargo builds a `cdylib` only from a library target.
- Neutral, because a desktop-only project may put `#[flui::main]` on its `fn main` or leave it off.

### Confirmation

- The compiler: `tests/ui/entry/pass/` (the attribute on a one-file `fn main`, an entry returning
  `()`) and `tests/ui/entry/fail/` (arguments, `async`, generics, attribute arguments, a non-`fn`
  item, an outcome that is not an `EntryOutcome`) in the facade's `thread_boundary_ui`.
- `flui-app`'s integration tests run `Application::run` and `run_resident` through
  `flui_app::testing::HeadlessRun::{start, start_resident}`; `capability_registration` checks the
  conflict as `LaunchError::CapabilityConflict` before the platform starts.
- The Android path is type-checked for `aarch64-linux-android`, and the nightly Android emulator
  job (`nightly-platforms.yml`) builds and starts the counter project `flui create` writes, whose
  only entry is its `#[flui::main]` function.

## Pros and cons of the options

### 1. `run_app(root)`, one builder, `#[flui::main]`

- Good, because `fn main` and `run_app` stay ordinary Rust, readable without the macro.
- Good, because the attribute is a no-op where it is not needed.
- Bad, because the `AndroidApp` crosses from `android_main` to the run through thread state.

### 2. `flui::main!(App)`

- Good, because a project would be one file.
- Bad, because a `cdylib` cannot hold a binary's `fn main`, so Android would still need a library
  target, and the macro would hide the run's configuration behind its own argument syntax.

### 3. A per-platform entry

- Bad, because it is the form this record replaces: a second entry that every Android project
  copies and that can drift from the desktop one.

## More information

`android-activity`'s `NativeActivity` glue calls an exported Rust-ABI `android_main(AndroidApp)` on
its own thread; Tauri's `mobile_entry_point` attribute and Slint's `android_main` follow the same
constraint.
