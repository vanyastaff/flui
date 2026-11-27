### Added

- **`flui-cli`**: `flui create` writes a `rust-analyzer.toml` that gives rust-analyzer its own
  target directory (`target/rust-analyzer`), in every editor. Its check on each save no longer
  holds the lock on `target/` that `flui run`'s rebuild of the same save waits on: with the
  check running beside it, five one-line edits of the counter rebuilt in a median 1.64 s
  (1.61–1.72 s) instead of 1.94 s (1.61–2.10 s); with no check, 1.49 s.

- **`flui-cli`**: a debug `flui run` of a desktop application on Linux or macOS shows each
  rebuild over the app's windows: a chip while it builds, and a panel of the compiler's errors
  with their `file:line:column` while the last build failed. The banner takes no input, and every
  compiler message still reaches the terminal as cargo writes it. `--no-build-overlay` turns it
  off (ADR-0395).
- **`flui-protocol`**: protocol 0.29 adds `build.report` and the method effect `annotates`, which
  an observer may call: it changes only what the development tools draw over a window, never the
  app. `MethodEffect::changes_app` is the one rule for what an observer may call and what a
  timed-out call may resend. A failed build's `Diagnostics` keeps at most `MAX_DIAGNOSTICS` and
  counts the rest, and a report with more is refused while it is decoded; protocol 0.30 states
  that bound in the schema.

- **`flui-cli`**: `flui run --json` reports `run.watch {paths}` once the sources are watched; a
  save made after it is a change the run sees.

- **`flui-cli`**: `flui mcp` gains `flui_reload`, which rebuilds the app and applies the build
  through the `flui run --agent` that started it, and answers what the reload came to: applied
  at a tier, or failed with the compiler's errors and their `file:line:column`, with the time it
  took. A `flui run --agent` that rebuilds on saves (hot reload, not `--release`) serves it at a
  private run endpoint of its own, names it `run` in `target/flui-agent.json` and reports it in
  `run.agent.ready` (ADR-0401).
- **`flui-protocol`**: protocol 0.31 adds `run.reload { at_least? }`, served by the new
  `ServedBy::Supervisor`, with `ReloadTier`, `ReloadParams`, `ReloadReport` and `Reloaded`.
  `ClientRole::may_call`, `launch::same_token` and `ServerLine::malformed` (with
  `envelope::Unread`) hold, for every server of the protocol, which
  calls a role may make, how a hello's token is compared and how a line that is not a request
  is refused.
- **`flui-app`**: on the desktop, `Application::run` and `run_resident` called while a
  development restart runs the application's entry hand the application to that restart and
  return `Ok(())` without starting a loop (ADR-0393 §4). Window options no window can be asked
  for fail there as at a start, and a second `run` in the same entry returns the new
  `LaunchError::AlreadyHandedOver`. No host starts a restart yet.

### Changed

- **`flui-cli`**: a project `flui create` writes declares no `cdylib`: the library is an rlib, and
  `flui build android` and `flui run` on an Android device compile it as the `cdylib`
  `NativeActivity` loads with `cargo ndk rustc -p <package> --lib --crate-type cdylib`. A desktop
  debug build no longer links a second copy of the framework into a shared library on every
  edit, which took 0.6–0.8 s of the 2.1 s from a one-line edit to a built counter. A project that
  still declares the `cdylib` builds the same library; one whose workspace's default members are
  several packages now builds the library of the package in the project's directory.
- **`flui-cli`**: `flui run` reports a save after 80 ms of quiet, instead of 500 ms on the
  desktop and 300 ms for an Android scene plugin, and the saves reported while a rebuild runs are
  one rebuild after it, not one each.
- **`flui-hot-reload`**: a host reads its worker or scene artifact every 50 ms instead of every
  500 ms (`timing::ARTIFACT_POLL`, `timing::WATCHER_POLL`), so a rebuilt worker is picked up up
  to half a second sooner. An artifact that failed to load is opened again once it changes, or
  after a second, not on every read.
- **`flui-cli`**: a save in `flui run`, and `r`, rebuild while the application keeps running and
  replace it only with a build that succeeded. A compile error leaves the previous build on
  screen instead of closing the window for the length of the build. Windows, which cannot
  overwrite a running executable, still stops the application first, and `R` still stops,
  rebuilds and starts.
- **`flui-cli`**: `flui run --json`'s `run.reload` event is the report `flui_reload` answers
  with, `{"outcome": {"applied": {"tier": …}} | "unchanged" | {"failed": {"diagnostics": […], "omitted": n}},
  "elapsed_ms": …}`, for every reload whatever started it, in place of `{"kind": …, "ok": …}`.
- **`flui-cli`**: `flui run` of a desktop application starts the executable its build wrote, with
  the library path `cargo run` sets (not its `CARGO_*` variables, `[env]` or runner), instead of
  running `cargo run`, which checked the sources again and could start a build nobody reported.
  A rebuild that produced the build the app already runs leaves the app running, so a save and a
  `flui_reload` of the same edit restart it once (not on Windows, which stops the app before
  each build). The reload's outcome is `"unchanged"` (protocol 0.34, which refuses older
  clients), and the `r` key reads "Rebuild, and restart if it changed".
- **`flui-view`**: a development reload hook reports the version of the code it loaded,
  `ReloadEvent::Loaded(CodeVersion)` with a `LayoutEpoch` and a `Revision`, in place of
  `ReloadEvent::Patched` (ADR-0393 §3). `DevReloadHook` gains `applied`, which the host calls
  with a `ReloadOutcome` once per UI runtime that reassembled and once per restart,
  `reassemble_requested` for the loaded code's own rebuild request, and `run_entry`. The loop
  keeps one record of versions: a version is applied once to each UI runtime, an idle window
  reassembles to the latest one, and a version reported twice changes nothing. A version of
  another layout is reported as `RestartFailure::NoApplication`, since nothing restarts yet.
- **`flui-cli`**, **`flui-hot-reload`**: `flui run` writes the worker's `.flui_worker_plugin`
  manifest as JSON, `{"path", "layout", "revision"}`, with a new revision for each staged build
  (`flui_hot_reload::WorkerManifest`). A worker loaded without a manifest has no version and is
  not reassembled, and `WorkerReloadDriver::loaded_version` reads the version of the loaded image.

### Removed

- **`flui-hot-reload`**: `timing::SOURCE_DEBOUNCE` and `timing::ANDROID_SCENE_DEBOUNCE`. Source
  watching is `flui run`'s, which keeps its own debounce; nothing read these.
- **`flui-hot-reload`**: `HotReloadTier` and `HotReloadOutcome`, which nothing read; the tiers
  are ADR-0393's and the host derives them. **`flui-runtime`**: `reload::ReloadTier`;
  `UiCommandSender::request_hot_reload(tier)` is `request_reassemble()`, the one reload a UI runtime
  runs.

### Fixed

- **`flui-hot-reload`**: a rebuilt worker is no longer missed by an idle host. The frame the
  artifact watcher's wake drew read the artifact only once the driver's poll interval had passed
  since the previous frame's read, so a rebuild that landed just after a frame went unseen until
  something else drew; and a rebuild that landed between making the hook and its watcher thread
  starting became the watcher's baseline and never woke the host.
- **`flui-cli`**: an application that exits while `flui run` rebuilds beside it is reported as
  that exit (`run.app.exit` with its code, and its last output), before the build that replaces
  it starts.
- **`flui-cli`**: on Linux a build no longer starts the next one. `flui run` took the read of
  `Cargo.toml` that every build makes, which Linux reports as an open, for an edit, so after each
  build it rebuilt and restarted the application again.
- **`flui-cli`**: `flui run` keeps seeing edits to `Cargo.toml` after an editor saves it by
  renaming a new file over it, and an overflowing event queue is a change to every watched path
  instead of a lost one. A worker host that exited while the worker built is reported as that
  exit before its successor starts.
- **`flui-cli`**: a dropped connection to an app's agent shuts its socket down (a Unix socket or an
  `adb` forward; not yet a Windows named pipe),
  so `flui mcp` reconnecting to a restarted app, and `flui run`'s build reports, no longer leave
  the old connection's socket and reader thread open until the app closes its end.
- **`flui-cli`**: `flui mcp` no longer loses a call that acts (`flui_act`, `flui_motion`) on the
  first try after the app restarted: a connection whose app closed its end sends nothing and the
  call goes on a new one, where it used to fail with the closed connection's error.
