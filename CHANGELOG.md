# Changelog

All notable changes to the FLUI workspace are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
All crates share `[workspace.package].version`, and every internal
dependency pins that exact version, so a published cohort can never mix
with a later one. The numbering starts at `0.1.0` where the public history
does: nothing was published before, and the beta status is stated in the
README rather than in a pre-release suffix that `flui = "0.1"` would not
match. Fine-grained phase history lives in
[`docs/archive/ROADMAP-TRACKER.md`](docs/archive/ROADMAP-TRACKER.md); this file records the
repo-consumer-visible summary.

## [Unreleased]

Workspace version bumped to `0.2.0-dev` to mark active development toward the next release;
every internal crate-to-crate pin moved in step (root `Cargo.toml` `[workspace.dependencies]`
plus each crate's own manifest, `Cargo.lock` regenerated). `docs/ROADMAP-TRACKER.md` and the
prior `docs/ROADMAP.md` moved to `docs/archive/` (historical, not a source of status); the live
`docs/ROADMAP.md` is now a short milestone table (B0–B4) pointing at the working roadmap
document. Beta-readiness audit reports landed under `docs/audits/2026-09-22-beta-audit/`.

### Added

- **`flui run --device <android serial>` and a Gradle-less `flui build
  android`** (`flui-cli`, `flui`, `flui-app`). The facade re-exports
  `run_app_android`, `run_app_android_with_config` and `android_activity`
  on Android (`flui-app` re-exports the crate), so a generated project's
  `src/lib.rs` carries an `android_main` while naming only `flui`. The CLI
  compiles that `cdylib` with `cargo ndk` and packages it with the SDK's
  build-tools (`aapt2`, `zipalign`, `apksigner`, debug keystore); a
  `platforms/android/gradlew` switches to Gradle. `flui run` installs with
  `adb install -r`, starts the `NativeActivity` and follows `logcat --pid`.
  See `crates/flui-cli/CHANGELOG.md`.
- **`TextEditingController::clear()`/`set_text()`** (`flui-widgets`).
  `set_text` replaces the whole buffer, collapses the caret to the end (a
  deliberate divergence from Flutter's `TextEditingController.text` setter,
  which collapses to an off-the-end `-1` sentinel that paints no caret at
  all — see the method's own doc), and clears any active composing region;
  `clear()` is defined in terms of it. No-ops without notifying when the
  value is unchanged, the same rule `insert_str`/`commit_text`'s
  unconditional-notify shape aside.
- **`TextField::on_submitted(Fn(&str))`** on both `flui_widgets::EditableText`
  and `flui_material::TextField` (the latter forwarding to the former).
  Fires on Enter while focused — never while composing (the IME owns
  Enter), never on a command chord (Ctrl/Cmd+Enter bubbles to an ancestor
  `Shortcuts` instead), never on Shift+Enter (reserved for a future
  multiline newline), and never twice for one held key (auto-repeat is
  consumed but does not resubmit).
- **Word-boundary text editing**: Ctrl/Alt+Left/Right,
  Shift+Ctrl/Alt+Left/Right, Ctrl+Backspace/Delete, and double-tap word
  selection all move/extend/delete/select by UAX #29 word segments now,
  not one grapheme cluster or an ASCII-whitespace run at a time
  (`flui-widgets`, `flui-painting`). `TextLayout::get_word_boundary`
  (`flui-painting`) moved off its old ASCII-whitespace-run scan onto
  `unicode-segmentation`'s UAX #29 segmentation; `TextEditingController`
  gained `move_caret_word_left/right`, `extend_selection_word_left/right`,
  `delete_word_backward/forward`; `GestureDetector`/
  `DoubleTapGestureRecognizer` gained `on_double_tap_down`, which
  `EditableText` composes around its pointer handlers to widen a
  double-tap's caret into the enclosing word. See
  `crates/flui-widgets/ARCHITECTURE.md`'s Mapping decisions #19–20 for the
  segmentation-algorithm and gesture-composition choices, including the
  known Thai/Lao/Khmer/Myanmar dictionary-segmentation limitation.

### Changed

- **`flui_widgets::TextField` renamed to `RawTextField`** (and
  `TextFieldState` to `RawTextFieldState`) — a breaking rename, sanctioned
  pre-1.0. `flui::prelude`'s `TextField` now names `flui_material::TextField`
  unconditionally, with no shadowing and no feature-dependent meaning (an
  intermediate revision had the Material type explicitly shadow the widgets
  one whenever the `material` feature was on, which — while it compiled
  correctly — violated Cargo's feature-additivity contract). See
  `ARCHITECTURE.md`'s `## Mapping decisions` entry for the full history.
  Nothing under `examples/`/`crates/flui-cli`'s templates referenced the
  renamed type; every existing `TextField` usage already meant the Material
  one and needed no changes.
- **MSRV 1.97 → 1.98**, and the policy changed with it: pre-1.0 the MSRV now
  tracks the latest stable release (bumped within a week of each new stable)
  rather than only when a stabilization is actually used; post-1.0 it will
  follow N-2. A new gate, `scripts/check-toolchain-consistency.sh` (wired
  into `just gate` and the `checks` CI job as `toolchain-consistency-check`),
  checks that `Cargo.toml`, `clippy.toml`, the `msrv` CI job, all five
  `flui-cli` project templates, the README badge, and `llms.txt` agree with
  `rust-toolchain.toml`'s channel, so the declaration can no longer drift
  silently across those files.

## [0.1.0] - 2026-09-21

First tagged release of the workspace. On crates.io this cut ships
`flui-cli` alone; the framework crates publish from a later tag, and until
then `flui create` pins this tag as a git dependency.

### Added

- **A form and an async section in the Material demo**
  (`examples/material_demo`): a validated `TextField` with an inline error
  and a Submit that enables only when valid, plus a scheduler-driven
  simulated fetch with loading, error → Retry, and cancellation on route
  pop — with facade-only headless tests for each, including that a fetch
  cancelled by navigating away never delivers.
- **`tests/agent_workflow.rs`**: the "agent workflow" acceptance test —
  mount the counter tree, dump the render diagnostics, find the button by
  its accessible label, tap its bounds through pointer replay, and assert
  the rendered count advanced, using only `flui::…`; a second test shows
  the actionable failure a missing label produces. `docs/testing.md` walks
  the same five steps. `flui::testing::rendering` now re-exports
  `render_diagnostics`.
- **`StateCell<T>` / `StateHandle<T>`** (`flui-view`, in `flui::prelude`):
  local state for a `StatefulView` that schedules its own rebuild. Replace the
  `Rc<Cell<T>>` field plus a hand-threaded `RebuildHandle` with one field,
  `bind(ctx)` it once in `init_state`, and write `count.update(|n| n + 1)` in a
  callback. The generated counter template and the multi-window and
  vertical-slice examples use it. Additive; `RebuildHandle` stays for widgets
  that choose their own `RebuildReason`.
- `AppConfig::with_*` and `Application`'s builder methods are `#[must_use]`:
  dropping the returned builder (`AppConfig::new().with_title("x");`) is now
  a warning instead of a silent no-op.

- **No blank window at launch on macOS** (`flui-platform`, `flui-app`): a window
  opened `visible: true` with the new `WindowOptions::reveal =
  WindowReveal::AfterFirstFrame` is ordered front fully transparent and made
  opaque only once the first frame has been presented into it (new
  `PlatformWindow::reveal_after_first_frame`, default no-op; `flui-app`'s
  desktop runner asks for the deferral and calls it on the first presented
  frame or after a one-second fallback when a frame ran and presented
  nothing). Before, the bare window background was on screen for as long as
  the GPU stack took to build — 2.81 s on a cold launch. The deferral is
  opt-in: the default `WindowReveal::AtOpen` keeps every direct
  `flui-platform` consumer's window visible at open, since only a frame-loop
  owner can report a first frame. Explicit `show`/`set_visible(true)`/
  `activate` reveal immediately. Other backends are unchanged.
- **Reachable from a screen reader** (`flui`, `flui-app`, `flui-semantics`,
  `flui-widgets`): the facade gains an `a11y` feature forwarding
  `flui-platform`'s AccessKit adapters (off by default; the Linux adapter
  carries a D-Bus stack), which no consumer could enable before. A labelled
  node with no role flag now resolves to AccessKit's `Label` rather than
  `GenericContainer` — which AccessKit's consumer filter hides from assistive
  technology, so every plain `Text` was invisible to VoiceOver — and
  `GestureDetector` publishes tap and long-press semantics actions, so a
  screen reader's activate gesture presses a button with no pointer event.
  `just macos-a11y` drives the generated counter through `AXUIElement`:
  the texts read as static text, `AXPress` on the button advances the count.
- **Android touches reach the framework** (`flui-platform`): the Android
  backend filled `PointerState::position` with physical pixels where every
  other backend — and the framework's reader — uses logical pixels, so on a
  density-420 emulator every tap landed past the viewport's edge and
  hit-tested nothing. It divides by the scale factor now, and
  `PlatformInput`'s doc states the contract. The first `MotionEvent` of the
  process is logged at `info` (later ones at `debug`) so a silent tap can be
  told apart from an undelivered one. Verified on an android-35 arm64
  emulator: two `adb shell input tap`s on the generated counter show «2».
- **Window-lifecycle probe** (`examples/lifecycle_probe.rs`,
  `just macos-lifecycle`): drives the running application's own window
  through miniaturize/deminiaturize, hide/unhide and a resize from AppKit
  and counts frames through each; the accepted run (zero frames while
  minimized or hidden, the panel rate after each restore, the resize
  reaching layout exactly) is recorded in `docs/BETA.md`.
- **The web canvas is the page's to size** (`flui-platform`): the browser
  backend's window is the canvas's CSS box, read from the live layout, and
  a `ResizeObserver` on the canvas plus the window's `resize` event keep the
  backing store at the device pixel ratio and dispatch a resize to the
  embedder on every change. A page-provided `#flui-canvas` keeps its own
  styling (`100vw`/`100vh`, a fixed frame, a flex child); a canvas the
  backend creates fills the viewport. Before, the backend pinned the canvas
  to `AppConfig::size` in inline CSS — overriding the page's `100vw`/`100vh`
  with an 800×600 box — and never dispatched a resize, so a viewport change
  or a zoom left the app at its first size. Verified in the desktop app's
  browser pane: the counter fills 1100×700, follows a resize to 980×1260 at
  DPR 2 (backing 1960×2520), and its button hit-tests after both.
- **Full frame rate for frames that do real work** (`flui-engine`): the
  swapchain's `desired_maximum_frame_latency` is 2 (wgpu's default) instead
  of 1. At 1, `examples/workload_probe.rs` — a Scaffold with a 2,000-row list
  and a text field on a 100 Hz display — presented every frame at exactly two
  periods (50 fps) while the bare platform pump ran 100 fps; at 2 it runs the
  full panel rate (scroll p99 10.1 ms). The value 1 had been kept for a
  live-resize argument whose in-process half was already measured absent and
  whose compositor half is unobservable; ADR-0029 carries the addendum.
- **Representative-workload probe** (`examples/workload_probe.rs`,
  `scripts/check-macos-workload.py`, `just macos-workload`): a self-driving
  scroll / type / idle workload with per-phase frame-timing JSON, RSS
  sampling, the real display period from CoreGraphics, and budgets declared
  before the first run; the accepted run is recorded in `docs/BETA.md`.
- **Text and Material/Cupertino buttons publish semantics** (`flui-objects`,
  `flui-material`, `flui-cupertino`): `RenderParagraph` describes its plain
  text as the semantics label with its text direction (Flutter's
  `describeSemanticsConfiguration`; an empty paragraph publishes no node —
  see `crates/flui-objects/ARCHITECTURE.md`), and `ButtonStyleButtonCore`
  wraps every button it composes in `Semantics(container, button, enabled)`,
  so a `Text("Increment")` inside an `ElevatedButton` is a labelled button
  node an assistive technology — or `A11yTree::find_by_label` — can find.
  The `flui create` counter template is screen-reader reachable out of the
  box; `tests/agent_workflow.rs` queries it with no explicit `Semantics`
  wrapper any more.
- **Grapheme-cluster text editing** (`flui-widgets`): `TextEditingController`'s
  `backspace`, `delete_forward`, `move_caret_left`/`right` and
  `extend_selection_left`/`right` step by extended grapheme cluster (UAX #29)
  rather than by Unicode scalar, so a family emoji, a flag or a letter with
  combining marks is one keystroke — Flutter's `characters` unit. An obscured
  field masks one bullet per cluster, keeping the mask in step with the caret.
  The step is resolved over the whole buffer (`GraphemeCursor`), so a caret
  that lands inside a cluster still steps to that cluster's edges, and
  `set_selection`/`set_caret_byte_offset` snap an in-cluster offset forward
  to a cluster boundary; only the IME preedit cursor keeps a mid-cluster
  position, since that is the input method's own state. Adds
  `unicode-segmentation` as a direct dependency (already in the graph
  through cosmic-text).
- **Automatic retry of a failed mobile surface recreation** (`flui-app`): when
  the Android or iOS runner is told its native window is available again and
  the wgpu surface rebuild fails for a reason other than the window not being
  there yet, the runner now retries under the same deadline-paced exponential
  backoff device-loss recovery uses (16 ms doubling to a 1 s cap, reset on
  success), driven through the platform's wake-deadline hook rather than a
  sleep on the event-loop thread. Previously the presentation stayed released
  and every later frame was skipped until some unrelated lifecycle event
  happened to re-emit the availability signal. The expected
  `SurfaceTargetUnavailable` answer — the signal arriving before a window
  exists — is never polled. Both runners settle each availability callback
  through one shared classification, so they cannot disagree about which
  failure is genuine.
- **CI lints the mobile runners and the facade for their own targets**: the
  `cross-typecheck` job now runs clippy on `flui-app` and `flui` for
  `aarch64-apple-ios` and `aarch64-linux-android`. Until this, the
  `cfg(target_os = ...)` runner code and the facade's target-gated re-exports
  had no compile gate at all; `flui` did not build for iOS because it
  re-exported `WindowPolicy` and `open_window`, which `flui-app` gates out
  there, unconditionally.

- `DisplayList::append_isolated` scopes each composed paint run, preventing a
  parent's canvas clip from leaking across `paint_child`. Serialized display
  lists now contain commands only and recompute bounds on input.
- Layer inspection's `clip_paths` returns borrowed paths (`Vec<&Path>`).
  Duplicate-leader rejection preserves the tree index when its debug assertion
  unwinds, and annotation diagnostics report the payload type.

- **Recovered lifecycle-panic diagnostics** (#561): `flui-view` exports the
  cloneable, non-exhaustive `RecoveredPanic`, `RecoveredAt`, and
  `LifecycleHook` record types. `BuildOwner::take_recovered_panics` and
  `WidgetsBinding::take_recovered_panics` are explicit `#[must_use]` drains;
  records name the panicking element, the mounted substitute, or a lazy
  delegate without overloading one id field with multiple meanings. Exact
  string-payload provenance is kept separately from the display-facing
  `FlutterError`; a non-string payload therefore remains redacted in an app
  report even when the embedder explicitly selects verbatim detail. Undrained
  records are discarded with one warning at the next frame start, bounding
  the producer even before a host forwards the diagnostics.
- **Held pointer input across failed frame commits** (#561): `flui-app` now
  queues addressed pointer events while a presentation is uncommitted, before
  hit testing or input-epoch stamping, and replays them through the ordinary
  pointer dispatch path after the next painted commit. This prevents a
  pointer sequence arriving during frame-failure recovery from targeting a
  tree that has not reached the screen; replay preserves event order and
  committed-tree hit testing, while gesture velocity remains based on delivery
  time.
- **Presentation-scoped frame-failure reporting and privacy controls** (#561):
  `flui-app` now exposes `FailureDisposition`, `FrameFailureDetail`,
  `PanicText`, and `SegmentPhase`, and re-exports `RecoveredAt` and
  `LifecycleHook` for handler-side matching. `FrameFailureHandler` receives
  contained lifecycle recoveries with their exact attribution and escaped
  segment panics with the phase that failed. Debug builds retain panic text by
  default; release builds redact it unless
  `AppConfig::with_frame_failure_detail(FrameFailureDetail::Verbatim)` opts in.
  Pipeline trace text passes through the same policy while the typed
  `RenderError` remains available to the handler.
- **Layout-poison retention fixtures** (#561): `a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero`
  and `a_leaf_that_never_committed_stands_in_with_zero` (`flui-rendering`'s `layout_poison` tests) tell a
  poisoned node's last committed geometry apart from the `Size::ZERO` stand-in, and go red when the poisoning
  pass is removed; a test-only `PipelineOwner::is_layout_poisoned` is the oracle.
- **`flui_foundation::panic` — one shared panic-payload classifier** (#561): `payload_text`
  extracts a caught panic payload's text (`&'static str` from a literal `panic!`, `String` from a
  formatted one, an honest `None` for anything else) and `is_internal_invariant` reads the
  `BUG:` prefix documented in `docs/PANIC-POLICY.md`. Every `catch_unwind` site that reads a
  payload's text — across flui-foundation, flui-platform, flui-app, flui-rendering, flui-view, and
  flui-widgets — now calls these instead of downcasting (or `Debug`-printing) the payload locally;
  no behavior change, one fewer place for the extraction to drift.
- **`flui create --no-check`** skips the `cargo check` that runs on a fresh scaffold. The check
  only reports (it never fails the command) and is a full cold build into the scaffold's own
  target directory — 236 s per template on the CI runner — so scripted, offline, or
  build-it-yourself-anyway use has no need for it. The command's boolean switches now live in
  `commands::create::CreateOptions`.
- **Runtime.1 conformance registry and public-API freeze gate** (#576):
  `docs/runtime-contract.toml` is the machine-readable inventory of every
  normative ADR-0027/0037/0029/0039 clause (state verified against current
  code, evidence or an owning issue) and of every public runtime/platform/
  scheduler/raster surface, with a stability classification (stable-candidate
  / experimental / transitional / removal-target), thread affinity, actual
  owner, and failure semantics. `scripts/check-runtime-conformance.sh`
  validates citation existence, evidence rules (documentation is never
  implementation evidence), forbidden retired identifiers, and registration
  nets for ambient singletons and SP-6 lock exemptions. Wired as
  `just runtime-conformance-check`, into `just ci`, and into CI's `checks`
  gate.
- **Generational presentation addressing** (#585): every frame is now
  addressed by `PresentationAddress` (`realm_id` + `presentation_id`,
  promoted to `flui-foundation`) instead of a bare `presentation_id`, since
  two different `UiRealm` incarnations can mint an identical `PresentationId`
  and only the full pair disambiguates a frame's owner. `PlatformWindow::id()`
  gives every window a stable identity; `SceneSnapshot` and every
  `RasterAck`/`PumpOutcome` variant (now individually `#[non_exhaustive]`)
  carry the full address; `RasterOwner` rejects a submit whose address
  doesn't match (`RasterSubmitError::AddressMismatch`) instead of accepting
  it on a partial (`presentation_id`-only) match. `WindowRegistry` is
  flui-app's sole native-window-to-presentation map. A new coalesced
  `SurfaceState` slot carries `SurfaceOutdated`/`DeviceLost` outside the
  lossy telemetry ack channel, since both are recovery-critical.
- **`OwnerPlatform` capability and typed owner-lane proxy** (#577, ADR-0039
  slice 2): `OwnerPlatform` (a `!Send + !Sync` owner-thread capability,
  minted only by a backend) and `PlatformProxy` (a `Clone + Send + Sync`
  cross-thread request capability) replace the ambient `&dyn Platform` every
  backend's `on_ready` used to hand out — the old shape stayed fully
  `Send + Sync` until a later trait split, so safe code on another thread
  could call an owner-affine method through it. `flui-foundation`'s new
  `ClaimSlot<T>`/`ClaimHandle<T>` (`Pending` → `Delivered` → `Claimed`, plus
  an `OwnerGone` terminal state and a `Future` impl) back `PendingWindow`'s
  at-most-once open-window reply, closing a window-leak-on-drop gap in the
  winit owner lane. `Platform::run` and its `on_ready` callback are now
  fallible (`anyhow::Result<()>`), so a bootstrap failure (window creation,
  GPU init, root-widget attach) stops the loop on every backend instead of
  running with no UI. `SharedPlatform` is the `Clone + Send + Sync`-safe
  residual of `Platform` a proxy can actually hold across threads.
- **Feature-selective facade, Material-first** (#574): `flui-material`
  (default), `flui-cupertino`, and the newly exposed `flui-localizations` are
  optional dependencies behind their own facade features; a module whose
  feature is off is absent from the graph, not an empty stub. `flui-hot-reload`
  leaves the production graph — optional in `flui-app` behind `hot-reload`,
  asserted absent from `flui-app`'s default `cargo tree` rather than assumed.
  The workspace's test-support crate is renamed to `flui-testing` (directory
  moved with history; `HeadlessBinding` keeps its name). `flui-app::theme` (`AppTheme`/
  `AppColorScheme`) is deleted outright per ADR-0042 — theming belongs to the
  design system (`ThemeMode` on `flui-material`'s `MaterialApp`), not the app
  framework. `just facade-combos` compiles each supported facade combination
  in isolation against `flui` alone, since a `--workspace` build proves
  nothing about the facade surface (feature unification would turn a broken
  combination green).
- **`flui-log` restored as a standalone, composition-only logging backend**
  (#571): `flui-foundation` no longer installs a process-global tracing
  subscriber (libraries emit through `tracing` only); `flui-log`'s
  three-outcome `SubscriberOwnership` (`Inherit`/`Auto`/`Install`) makes
  ownership explicit and never panics on an already-owned slot, replacing an
  `init()` that used to panic the moment a host process already owned one.
  Fixes three real defects found while restoring it: a stray `LevelFilter`
  ceiling that silently discarded events `RUST_LOG` had just selected, an
  illegal Apple subsystem identifier synthesized from an arbitrary app
  display name, and a logcat field-renderer separator that never fired when
  the message arrived first — untested because its tests lived inside a
  `cfg(target_os = "android")` island that never compiled anywhere.
  `docs/workspace-layers.toml` gains `allowed_dependents`, so a normal edge
  into `flui-log` from anything but `flui-app`, `flui-cli`, or the facade now
  fails `just inventory-check`.
- **`cargo-deny` CI job** (merge-blocking): the in-repo `deny.toml` was never
  executed in CI; the first wired run surfaced four real advisories —
  `anyhow` 1.0.102 unsound `downcast_mut` (RUSTSEC-2026-0190),
  `crossbeam-epoch` 0.9.18 invalid deref (RUSTSEC-2026-0204), and a
  `quick-xml` DoS pair (RUSTSEC-2026-0194/0195). Three fixed by lockfile
  bumps; the transitive quick-xml pair (build-time Wayland scanner) and the
  unmaintained `ttf-parser` notice carry documented ignores. `just deny`
  added.

- **CI gates**: integration tests now run in CI (previously `--lib` only —
  the Core.0/Core.2 exit-gate suites in `crates/*/tests/` were never
  executed); new `doc-test` job runs every rustdoc example; new `msrv` job
  verifies the declared MSRV floor; new advisory `miri` job checks the
  `flui-rendering` subtree arena (the workspace's densest `unsafe` hot spot);
  the `gpu-test` WARP readback suite is promoted from advisory to
  merge-blocking after 3 consecutive green full-suite runs.
- **Panic policy** ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)):
  `Result` for caller-triggerable failures, `expect("BUG: <invariant>")` for
  internal invariants, enforced by `clippy::unwrap_used` at workspace level
  (tracked crate-level opt-outs burned down per quality wave).
- This changelog.
- **Clip-producer layer-update test support** (#996): `flui_layer::testing::inspect::clip_rects`/
  `clip_rrects` walk a `LayerTree` for its `ClipRect`/`ClipRRect` layers in pre-order;
  `flui_widgets::testing::LaidOut::clip_rrect_layers` exposes the latter to widget tests. New
  Criterion groups `paint/clip_rrect_radius_change` and `paint/clip_path_token_change`
  (`flui-rendering`'s `paint` bench) measure the clip producers against the same update/repaint
  ratio the opacity/transform/rotated-box groups already cover. Two new
  `tests/composited_layer_update_readback.rs` cases pin the clip patch against a forced repaint
  pixel-for-pixel at a clipped corner.

### Changed

- **`flui-cli` reads like a binary, not a library** (`flui-cli`). Error
  messages now follow the `thiserror` convention — lowercase, no trailing
  period, and the wrapping variants (`I/O error`, `build failed`, …) no
  longer repeat the cause that the `Caused by:` chain already prints; a
  command that dies by signal says so instead of printing `None`. Every
  `pub` item is `pub(crate)` (nothing outside the binary can see it), which
  let the compiler find the library-era leftovers that went with it: a
  `prelude`, a sealed trait with nothing to seal against, unused builder
  methods, error variants no code constructs, `new_unchecked` constructors
  that bypassed validation, 47 doc examples no tool ever compiled, and
  `flui.toml` sections (`[assets]`, `fonts`, `lto`, `opt_level`, per-mode
  build tables) that nothing read but `flui platform` wrote back into the
  user's file. `flui.toml` now models exactly the keys the README documents;
  unknown keys from older files are ignored.
- **`flui-cli` build pipeline: one streaming process runner, no builder
  trait** (`flui-cli`). The `PlatformBuilder` trait had four implementations
  and no polymorphic caller (`flui build` matches on the target), and it hid
  what the compiler now reports: two builders whose environment check and
  staging step never touched `self` or awaited anything, and two dead
  `types.rs` methods. The platform tools (Gradle, `wasm-pack`, `xcodebuild`,
  `cargo ndk`, `adb`) go through one `process::run` that kills the child
  when the build is cancelled, `JAVA_HOME` reaches the Gradle wrapper (it
  was resolved and never used, while the warning promised the APK step
  would be skipped — now it is), and the iOS simulator probes use the
  bounded runner in `proc.rs` instead of a fresh tokio runtime per call.
- **`flui create` builds without the framework on crates.io** (`flui-cli`).
  A generated project's `flui` dependency is the git tag matching the CLI's
  version (`{ git = "https://github.com/vanyastaff/flui", tag = "v0.1.0" }`,
  `features` preserved) until `FRAMEWORK_ON_CRATES_IO` in
  `templates/source.rs` is flipped, which is the CLI release after the
  framework's first publication; `--local` is unchanged. This lets
  `flui-cli` ship alone: the framework's 25-crate publish closure no longer
  gates it.
- **`flui-cli` release gates** (`flui-cli`, `.github/workflows/ci.yml`,
  `.github/workflows/weekly.yml`, `docs/workspace-layers.toml`).
  `cargo publish --dry-run -p flui-cli` passes: the `flui-hot-reload`
  dev-dependency is path-only (Cargo drops it from the published manifest,
  and the layers inventory records it as checkout-only), so the CLI no
  longer waits for the framework to be on crates.io. `cargo deny` is clean.
  New CI job `cli-macos` runs the CLI suite on macOS, the only execution of
  its simulator, Xcode, `.app` staging and termios arms; the weekly
  workflow gains `cli-live-build`, which scaffolds a project against the
  checkout and builds it for desktop end to end.
- **`cargo flui` and prebuilt CLI binaries** (`flui-cli`,
  `.github/workflows/release.yml`). `flui-cli` ships a second binary,
  `cargo-flui`, an exec shim over the `flui` installed beside it, so
  `cargo flui run` is the same CLI with the same exit codes. A `v*` tag
  builds `flui-<target>` archives for five targets with the workspace
  release profile (thin LTO, ~3 MiB versus ~4 MiB from `cargo install`) and
  drafts a GitHub release with `SHA256SUMS`; `[package.metadata.binstall]`
  points `cargo binstall flui-cli` at them. Publishing the draft and
  `cargo publish` stay human steps.
- **Dependency audit with `cargo shear`** (`flui-cli`, `flui-hot-reload`,
  `justfile`). `just shear` runs the feature-aware unused-dependency check.
  `flui-cli` drops clap's `cargo` and `env` features (no `crate_*!` macro,
  no `#[arg(env)]` in the code) and declares `serde` locally with `derive`
  only; `flui-hot-reload` drops `flui-types`, an optional dependency of
  `app-plugin` that nothing imported. `cargo outdated` finds every direct
  dependency of the CLI on its latest release.
- **`flui-cli` draws its own terminal output** (`flui-cli`). `cliclack` is
  gone: it brought 42 of the CLI's 116 crates — ICU text segmentation with
  its data tables and proc-macros, to word-wrap prompt text — for a dozen
  lines of glyphs and a spinner, which now live in `ui.rs` over `console`.
  The prompts of `flui create` and `platform remove` run on `dialoguer`
  without its default features (one extra crate). `pollster` is gone too:
  the build command already enters a tokio runtime, so its handle drives
  the async builders. The CLI's normal dependency graph is 76 crates, down
  from 116; the output keeps the same shape (a bar down the left, one glyph
  per line kind), the same `--json`/`--quiet`/`--color` behaviour, and the
  spinner prints its start and end lines once when stderr is not a terminal.
- **`flui-cli` has no internal dependency and no logging framework**
  (`flui-cli`; breaking for `RUST_LOG` users). The CLI dropped `flui-log`,
  `tracing` and `tracing-subscriber`. With no framework crate in its graph a
  `RUST_LOG` filter could only select the CLI's own 57 call sites, 28 of them
  `INFO` lines inherited from the build library that duplicated what the
  commands already report, so stderr carried two voices. Warnings and errors
  now go through the same `ui::` functions as every other line; diagnostics
  (the commands flui runs, the probes it makes, the paths it skips) are
  dimmed `debug:` lines on stderr under `-v` only, in JSON mode too, and a
  test pins that they never reach stdout. `RUST_LOG` is no longer read.
  `cargo install flui-cli` compiles no FLUI code: the normal dependency graph
  is down from 135 crates to 116, all external.
- **`flui-devtools` is only what exists** (`flui-devtools`; breaking). Default
  features now enable the crate's three modules (a crate that exists only to
  provide them shipped with nothing on by default), so `cargo doc` also
  documents them. `DevToolsConfig` is `ProfilerConfig` with just the two
  fields the profiler reads; the unread `profiling_enabled`,
  `inspector_enabled` and `target_fps` are gone, as are the unused
  `FrameNumber`/`Timestamp`/`DurationNanos` types, the `VERSION` constant, an
  unused `windows-sys` dependency, and `FEATURES.md`. The README describes
  the profiler, the timeline and the observation counters, and no longer
  promises an inspector UI, a network monitor or a memory profiler.
- **`flui-build` is gone; the build pipeline is a module of `flui-cli`**
  (`flui-cli`, `flui-hot-reload`, `flui-devtools`; breaking for anyone who
  depended on those surfaces). `flui-build` had one consumer, the CLI, and no
  framework dependency, so it moved to `crates/flui-cli/src/build/`. The
  dev-loop `SourceWatcher` moved from `flui-hot-reload`'s `source-watch`
  feature to `crates/flui-cli/src/watch.rs`: it is dev-machine code, and
  keeping it in the runtime crate made `cargo install flui-cli` compile the
  rendering stack. The CLI now links no framework crate but `flui-log`; its
  normal dependency graph fell from 199 crates to 135, and it no longer has
  to wait for the framework's own release to be published. The two env-var
  names shared with the runtime (`FLUI_HOT_RELOAD`, `FLUI_WORKER_PLUGIN`) are
  pinned by a dev-dependency test. `flui-hot-reload` loses the `source-watch`
  feature and `dev` module; `flui-devtools` loses the `hot-reload` feature and
  `HotReloader` (a callback wrapper with no consumer). Merging also exposed
  library API the CLI never used, deleted rather than gated: the progress
  reporter and `indicatif`, the build-output parser, `BuilderContextExt`,
  `--features` plumbing no command could set, and the builders' `clean`.
- **`flui` CLI brought to release quality** (`flui-cli`). One output policy
  for every command: human text on stderr, `--json` NDJSON events on stdout
  (`doctor.check`, `device`, `run.app.log`, `build.done`, …), `--quiet`,
  `--color auto|always|never` honouring `NO_COLOR`/`CLICOLOR_FORCE`, and
  `--non-interactive` (implied by `CI`, `FLUI_NON_INTERACTIVE`, or a
  non-terminal stdin) that turns every would-be prompt into an exit-7 error
  with the flags to pass. Exit codes are now a documented contract (0/1/2/3
  environment/4 build/5 device/6 not a project/7 needs a terminal/130 Ctrl-C).
  `flui run` gained hot-keys (`r` reload, `R` restart, `c`, `h`, `q`), a
  Ctrl-C path that always stops the app first, `--device` resolution against
  the same discovery `flui devices` uses (unknown → exit 5, Android/browser
  → exit 2 with the command to use instead), and a library-crate refusal.
  `flui doctor` models checks as data with `[✓]/[!]/[✗]` lines and fix hints,
  distinguishes required from optional toolchains, checks the MSRV, and
  `--fix` installs missing `rustup` targets. `flui devices` and
  `flui emulators` share one device model, read simulators from `simctl`
  JSON, and print the ids `--device` takes. `flui build` prints every artifact
  with its size and a timing line. `flui create` plans files before writing
  them (`--dry-run`), and every listed template is real: `counter`, `basic`,
  `empty`, `widget` (`--lib`), plus `--hot-reload`. The CLI README documents
  all of it, including a Flutter command mapping. Breaking: the placeholder
  templates `todo`, `dashboard` and `plugin` are gone; `flui run --scene` no
  longer defaults `--package`/`--scene-crate` to the author's project; the
  no-op `build --split-per-abi`/`--optimize-wasm` flags are removed;
  `flui upgrade` updates dependencies only unless `--self` is given, and
  `--self-update` is now `--self`; `flui test --platform` (never implemented)
  is gone; the `flui devtools` placeholder command is removed until a server
  exists.

- **Version `0.1.0`, exact cohort pins, and archives that carry only
  what a consumer compiles.** Every internal `path` dependency requires
  `=0.1.0` (the cohort was first cut as `0.3.0-beta.1` and renumbered before
  any publication, see the header). The `flui` facade package
  declares an `include` list (its archive went from 666 files — docs, scripts,
  CI, editor and research directories — to 67), and every published crate
  ships `LICENSE`, `LICENSE-APACHE` and `NOTICE`, which the archive check now
  requires. `just release-consumer-check` packages the release set, vendors
  every third-party dependency, installs the archives as a Cargo directory
  source and builds and tests a `flui create` project against them offline —
  the first proof that a registry consumer can build without this checkout.

- **Typed window errors on the facade** (`flui-app`, `flui`): `open_window` and
  `open_secondary_window` return `Result<(), AppWindowError>` instead of
  `anyhow::Result<()>`, so a caller can match on what failed. `AppWindowError`
  gains `AdmissionClosed` (the application is quitting or the loop is gone),
  `NoOwnerLoop` (called off the owner thread) and `UnsupportedPolicy { reason }`
  (`SharedRealm` where no realm is hosted, or with mounted content). The
  facade now re-exports `AppWindowError`, `AppRunError`, `AppControlError`,
  `Application`, `AppHandle`, `StartupWindow` and `run_app_with_config` at
  `flui::` (and `AppWindowError`/`run_app_with_config` in `flui::prelude`),
  so a sole-`flui` consumer can reach the resident-application builder and
  name every error the entry points produce without a second dependency.
  **Breaking:** code that relied on `anyhow::Error` from those two functions
  must switch to `AppWindowError` (or `.map_err(anyhow::Error::from)`).

- **Build-side lifecycle and reconciliation failures are contained at the
  failing child** (#561): a parented state whose `init_state` or
  `did_change_dependencies` panics is finalized and replaced by an `ErrorView`
  at its own slot, then the build drain continues; a root still propagates
  because it has no parent slot to repair. Fresh child creation and
  `GlobalKey` retake/update failures use the same bounded substitution in both
  sparse and dense reconciliation, so healthy siblings continue and recovery
  records identify the panicking element or mounted substitute without a
  duplicate record. The catch region, not the panic text, selects recovery:
  `BUG:` only marks the record as an internal invariant. A failing custom
  recovery-view factory and framework bookkeeping outside the documented
  child regions are not locally recovered; they propagate to the
  presentation boundary rather than being misreported as recovered.
- **A `ViewState::dispose`/`activate`/`deactivate` panic, and a
  `RenderView::did_unmount_render_object` panic, are contained per element,
  not per frame** (#561): `StatefulBehavior::on_unmount` catches a panicking
  `dispose` and `StatefulBehavior::on_deactivate` catches a panicking
  `deactivate` instead of letting the panic unwind out of
  `BuildOwner::build_scope` / `finalize_tree` — the tree-side teardown (slab
  slot freed or parked inactive, `GlobalKey` unregistered, inherited edges
  released) still completes either way, and `ElementCore::deactivate`'s
  lifecycle flip to `Inactive` still runs right after a contained `deactivate`
  panic returns. `activate` records at the exact descendant whose hook
  panicked, then rethrows into the retake window so the relocation is undone
  without a duplicate record. A state whose `init_state` never completed
  (removed before its first build, or `init_state` itself panicked) receives
  none of `activate`, `deactivate`, or `dispose`.
  **Breaking:** `ElementBase::{activate, deactivate}` and `ElementBehavior::{on_activate,
  on_deactivate}` (public traits) now take an `owner: &mut ElementOwner<'_>` handle, mirroring
  `mount`/`unmount`'s existing shape, so the deactivate-side catch has an owner to report through.
  Every implementor lives inside `flui-view` itself (production code and test fixtures); there is
  no implementor anywhere else in the workspace, so there is no known external implementor to
  migrate.
  `RenderBehavior::on_unmount` contains a panic from `did_unmount_render_object` the same way,
  inside the existing `PipelineOwner::with_mut` closure — the recorded panic is only pushed after
  the closure returns, since the owner handle must not be re-entered while the cell borrow is
  live — and `remove_render_object_from_tree` still runs unconditionally after, whether or not
  the hook panicked. `AnimatedBehavior` closes the parallel `listenable()` seam by caching the
  `Arc<dyn Listenable>` it subscribed to (`subscribed: Option<(Arc<dyn Listenable>, ListenerId)>`,
  replacing the bare `ListenerId` it used to hold) instead of catching a panic there: `on_unmount`
  removes through the cached `Arc` and never calls `core.view().listenable()` again, and
  `on_view_updated` compares the new view's `listenable()` against the cached `Arc` by
  `Arc::ptr_eq` instead of re-reading `old_view.listenable()` — one user call per update instead
  of two. `listenable()` is the only handle to the listenable being cleaned up, so a catch around
  a second call to it at unmount could never make removal safe; caching closes the seam by never
  calling it there at all. **Breaking:** storing `Arc<dyn Listenable>` means
  `AnimatedBehavior<V>` no longer auto-implements `UnwindSafe` or `RefUnwindSafe`; callers that
  cross an unwind boundary must establish safety explicitly, as the framework's narrow
  containment windows do with `AssertUnwindSafe`.
- **Breaking frame-failure API reshape** (#561):
  `FrameFailureKind::SegmentPanic.message` changes from `Option<Box<str>>` to
  `PanicText` and the variant gains `phase: SegmentPhase`; downstream matches
  must adopt the new fields and use `..` for forward-compatible destructuring.
  `FrameFailureReport` additionally exposes `disposition`, and the
  non-exhaustive `FrameFailureKind` gains `RecoveredPanic` for lifecycle
  failures repaired without dropping the frame. This is the active-development
  equivalent of a `0.2.0` to `0.3.0` API change; no version or tag is cut here.
- **`PaintEffects` — one value for a render object's own paint effects**
  (#996): `RenderBox`/`RenderSliver`/`RenderObject::paint_effects(size)`
  returns `PaintEffects { opacity, clip, transform }` with a fixed nesting
  (opacity outermost, then clip, then transform); the value is read by the
  paint walk, the composited-layer-update patch arm, and the default
  `apply_paint_transform`. The six existing producers — `RenderOpacity`,
  `RenderAnimatedOpacity`, `RenderSliverOpacity`, `RenderSliverAnimatedOpacity`,
  `RenderTransform`, `RenderRotatedBox` — migrated behaviour-neutrally.
  `ClipPathLayer` and `RenderClip<Path>` now share a fixed path as one
  `Arc<Path>` (`ClipGeometry` gains an associated `type Stored`); a
  size-dependent clipper can be reported as data through
  `PaintClip::PathTarget` and resolved by the walk through
  `resolve_path_clip`, rather than the producer running the clipper itself.
  The composited-layer-update patch arm rebuilds a boundary's effect layers
  in capture order.
- **`RenderClip<S>` (rect/rrect/oval/path) and `RenderFlow` report their clip through
  `paint_effects` instead of pushing it as a fragment scope** (#996): `RenderClip`'s five
  setters (`set_clip_behavior`, `set_clip_shape`, `set_border_radius`,
  `set_path_clip_source_token`, `set_path_clip_target`) now report `COMPOSITED_LAYER_UPDATE`
  (`| SEMANTICS` on the four that change the resolved geometry) instead of `PAINT` — a clip
  property change under a retained boundary patches the layer without repainting the subtree
  (measured 220x at 1000 inline nodes for a `ClipRRect` radius change, 1.71x layered; 192x/1.69x
  for a `ClipPath` token change, resolving the registered clipper once on either arm). A
  token-driven path clipper is carried as `PaintClip::PathTarget` and resolved once, by the paint
  walk, never on a coordinate query. `RenderFlow`'s clip stays gated on `Clip::None`, so
  `set_clip_behavior` still reports `PAINT | SEMANTICS` — the production type behind the
  structural-refusal oracle.
- **A node's effect descriptor now builds inside the paint guard; a panic while
  building or patching one poisons the frame** (#996): the paint walk reads a
  node's `paint_effects` — and resolves any `PaintClip::PathTarget` it
  reports — *after* the walk's `skip_paint`/`needs_layout`/sliver-visibility
  gates, inside the same `catch_unwind` as `paint_raw`, so a gated-out node no
  longer builds a descriptor it will not use; a panic there surfaces as
  `RenderError::Poisoned { phase: PoisonPhase::Paint, .. }` instead of an
  unwind that leaves the phase never exited. `layer_patches_for` (the
  composited-layer-update patch arm) is fallible for the same reason: a panic
  while rebuilding a boundary's own effect layers poisons the frame with
  phase `PoisonPhase::LayerUpdate` rather than falling back to a repaint —
  which would call the same panicking `paint_effects` a second time — and the
  queued update survives on the node for the retry.
- **Toolchain 1.98.0 → 1.98.1 (development pin only; MSRV floor stays 1.97).** The current
  stable point release (2026-09-01); CI's `stable` jobs already floated to it, so local and CI
  were a point release apart. Verified with `cargo check --workspace --all-targets`, clippy at
  `-D warnings`, and `cargo fmt --check` before bumping.
- **`anyhow` retired from `flui-platform`'s public API** — the last workspace
  library exposing it. A new typed taxonomy (`flui_platform::PlatformError`:
  `Init` / `EventLoop` / `Bootstrap` / `AppPath` / `Dialog`, thiserror,
  `#[non_exhaustive]`) covers `current_platform()`, `Platform::run`,
  `Platform::app_path`, and the file-dialog methods; `Platform::open_window`
  adopts the existing `OpenWindowError` capability taxonomy (gaining an
  `Unavailable` variant for event-loop lifecycle refusals — the growth
  ADR-0039 forecast for the slice-3 method adoption), and
  `PlatformReadyCallback` now returns the opaque
  `BootstrapError = Box<dyn std::error::Error + Send + Sync>` (embedder
  bootstrap is application-land; `Platform::run` wraps it as
  `PlatformError::Bootstrap` with the embedder error as `source`, preserving
  the loop-also-failed-while-unwinding case in a `loop_error` field).
  `anyhow` is now a dev-dependency of `flui-platform` (tests/examples only);
  `flui-app` keeps `anyhow` internally and converts via `?`.
- **Structural refactor pass (round 4): the debts the repo's own docs named,
  executed or honestly retired.** `flui-app`'s `runner.rs` — 11,616 lines,
  66% of the crate together with `ui_realm.rs`, and the workspace's largest
  file by a factor of five — is now the `app/runner/` module directory: ten
  files split along the file's own region banners (platform entry points
  per target, the loop-scoped host, realm install/dispatch/teardown,
  lifecycle ladder, frame pacing, device recovery, the secondary-window
  seam), move-only, every test module staying beside the code it pins, no
  resulting file over ~630 non-test lines. The frame-ordering mechanical
  guards now scan all ten sources; `docs/runtime-contract.toml`'s 64
  runner-file evidence entries are re-pointed to the exact new homes. The
  standing SP-3 parallel-type debt shrank by 35 markers: the nine
  gesture-detail types double-defined across flui-types/flui-interaction
  are consolidated on the interaction side — the flui-types copies had zero
  consumers, and merging the live pipeline downward would have lost
  observable callback payload (device kind, end positions, focal/scale/
  rotation), so the split for the genuinely shared details is now a
  documented invariant instead of an apology; `AnimatableExt`'s two
  definitions inside flui-animation merged; the macOS backend's duplicate
  `WindowId` became a re-export of the canonical `traits` definition;
  flui-scheduler's `Percentage` (an `f64` 0–100 budget readout colliding
  with flui-geometry's `f32` 0–1 layout fraction) is renamed
  `BudgetPercentage`; and port-check trigger 10 gained a `Sealed` carve-out
  for the eleven idiomatic per-module sealing traits that were never debt.
  flui-engine's long-standing "audit the painter caches for deletion"
  entry (budgeted at ~1,955 LOC) resolved the honest way: traced from the
  Renderer/Backend entry points, all four subsystems (`texture_cache`,
  `external_texture_registry`, `path_cache`, `multi_draw`) are live on
  production paths — they had moved homes during the painter split, and
  the entry's premise was stale — so the deletion is recorded as
  won't-do-with-evidence, the one genuinely dead helper
  (`ShaderCache::clear`) is gone, and the four audited areas now carry
  zero `allow(dead_code)` at any scope. The eight dated debt markers due
  2026-09-22 are each resolved per their own contract (verified-and-
  deleted, or re-dated with fresh evidence); paint interning's three
  stale doc twins now record the landed `Arc<Paint>` + structural-dedup
  shape; ROADMAP-TRACKER's H10 row is narrowed to the truth (wgpu 30
  done, winit 0.31 open); and the last banned process-marker comments in
  flui-engine test files are rewritten as plain-English invariants.
- **Workspace-wide dedup/refactor pass (round 3): every deferred item from
  round 2's assessment, closed.** flui-engine's `TexturePool` drops its
  `Arc<Mutex<TexturePoolInner>>` — the pool owns its inventory directly and
  hands out `PooledTexture`s carrying an `mpsc` return channel, so a dropped
  texture rejoins the free list at the pool's next `&mut` operation instead
  of through a lock (`Sender` keeps the type `Send`; no consumer signature
  changed). This diverges deliberately from the backlog's original
  explicit-`release` sketch — pooled textures ride inside `draw_order` and
  blend-op values whose drop order is not a call site — and the divergence
  is documented in flui-engine's ARCHITECTURE.md; port-check trigger 7's
  `texture_pool.rs` exemption glob is deleted per its stated obligation.
  flui-platform collapses the ~60 verbatim `on_*` callback setters across
  its six `PlatformWindow` backends onto one
  `impl_window_callback_setters!` macro, hoists the thrice-copied
  `PROCESS_START`/`event_timestamp_ns`/`primary_mouse_info` block into
  `shared/events.rs`, and exports the headless `MockWindow` type so
  downstream tests can downcast to it. flui-app replaces fourteen of its
  sixteen hand-rolled `RasterBackend` test doubles with one configurable
  `TestRasterBackend` (scripted per-frame outcomes; the two doubles that
  exercise the private `DeviceRecovery` seam stay hand-written and say why),
  and its six `PlatformWindow` stubs with one builder-style `TestWindow`
  (kept crate-local rather than adopting `MockWindow`: that double is
  minted by a live `HeadlessPlatform` and drags in window-tracking and
  exit-policy machinery that state-level unit tests do not want). Finally,
  the three drifted copies of the widget mount harness are one module:
  `flui_widgets::testing` (moved from flui-widgets' `tests/common`, gated
  `#[cfg(any(test, feature = "testing"))]` — the feature name port-check
  trigger 11 sanctions) absorbs the material/cupertino extras
  (`count_elements_by_view_type`, `children`, ErrorView-tolerant root
  resolution, a new Option-returning `try_find_by_render_type`), and both
  design-system crates' `tests/common` shrink to re-export shims. The
  unification is a semantics upgrade, not just dedup: material/cupertino
  tests now get the fresh-pointer-id-per-contact dispatch flui-widgets
  already had, which exposed nine material tests driving a contactless
  hover as a pressed-button move — a stream no platform emits — now
  corrected to `dispatch_pointer_hover`. The per-contact id allocation and
  the 8ms pointer-sample clock policy are shared with `test_harness.rs`
  via `testing::PointerContacts`; `Harness` itself deliberately stays a
  separate type (it exposes element-tree probes and withholdable IME/
  post-frame capabilities the mount harness does not). `flui-testing`
  stays out of every production graph (`cargo tree --edges normal`
  verified), with the new feature-only edge registered in
  `docs/workspace-layers.toml`.
  flui-rendering's `RenderNode` collapsed 39 identical Box/Sliver
  match-delegations onto one local `with_entry!` macro (~110 LOC), and its
  42-file integration binary gained the `tests/common` module its per-file
  scaffolding copies (7× `laid_out`, 5× `sliver_geometry`, constraint
  helpers, the `Boxed*Object` aliases) had been begging for; six orphaned
  snapshot files whose source test moved to flui-objects long ago are gone
  along with the unused `insta` dev-dependency. flui-objects gained
  `forward_single_child_box_layout!`/`forward_single_child_box_hit_test!`
  beside the existing query-forwarding macro (23 verbatim proxy bodies
  replaced; constraint-transforming implementations stay hand-written).
  `ChangeNotifier` is re-seated on `Notifier<()>` — the snapshot/ordering/
  `catch_unwind` firing discipline now lives once in `flui-foundation`'s
  generic channel, with `ChangeNotifier` keeping only its Flutter-parity
  seams (branded use-after-dispose message; `remove_listener` tolerating a
  disposed receiver via the new `Notifier::remove_even_if_disposed`).
  `flui-layer` gained the `gen_layer_from_impls!` macro its backlog called
  for (18 hand-written `From<XxxLayer>` impls collapsed, plus the previously
  missing `From<PerformanceOverlayLayer>` closed and its one hand-boxed
  construction site in `flui-app` simplified). `flui-view` gained
  `single_child_view_children!`, replacing 51 verbatim
  `has_children`/`visit_child_views` blocks across
  flui-widgets/-material/-cupertino. flui-painting's text layout finished
  the cosmic-text 0.19 migration semantically, not just syntactically:
  `Buffer::new_empty` drops the wasted empty-string shape pass at both
  construction sites, the global `FONT_SYSTEM` lock now brackets only shape
  passes (the lazy setters run before it), and the verbatim metrics fold
  shared by `TextLayout::metrics` and `measure_text` lives once
  (`metrics_from_shaped_buffer`). `Color::to_f32_array` is a `const`
  delegation to `to_rgba_f32_array` instead of a copy, and the two
  ignored HSL/HSV round-trip tests whose "not implemented" premise was
  false (the `From` conversions exist) now run.
- **Doc truth-sync across crates.** Every claim that routed current behavior
  through the deleted `AppBinding` now names the real successor
  (`UiRealm::draw_frame` / `render_frame_entered` /
  `handle_input_addressed`) across ~30 sites in
  flui-view/-testing/-widgets/-material/-cupertino/-app/-platform;
  deliberately historical "retired `AppBinding`" anchors stay.
  flui-layer's ARCHITECTURE.md dropped its stale doctest backlog (the
  `px()`-wrap sweep landed long ago; doctests are green) and records the
  `From`-impls macro as done. flui-foundation's notifier docs now state the
  round-N-vs-round-N+1 rule on both channels, closing that backlog entry.
  flui-painting's ARCHITECTURE.md reflects the 0.19 lock discipline and
  newly files the UAX #29 word-segmentation entry `get_word_boundary`'s
  doc always claimed existed.
- **flui-engine: single homes for the GPU rituals the wgpu 30 bump touched at
  every call site.** The version bump adapted each site in place; this change
  deduplicates the repeated shapes. `wgpu/adapter.rs` now owns the production
  acquisition policy — `trusted_adapter_options` (the one
  `RequestAdapterOptions`, carrying the `apply_limit_buckets: false` rationale
  once instead of five times), `request_flui_device` (the
  capability-negotiated `DeviceDescriptor`), and `request_offscreen_gpu` (the
  instance → adapter → capabilities → device sequence that
  `Renderer::new_offscreen`, the offscreen half of `recover`, and
  `GpuServices::resolve_offscreen` each previously spelled out).
  `wgpu/test_support.rs` (gated on `testing`) replaces the per-file
  GPU test scaffolding — adapter/device acquisition under six different names,
  render-target creation, clear passes, and the padded-row staging readback —
  that ~25 test files each carried a copy of; per-suite oracles and scene
  builders stay local. The ten near-identical unit-quad pipeline constructors
  in `pipelines.rs`/`effects_pipeline.rs` collapsed onto one
  `QuadPipelineSpec` + `create_unit_quad_pipeline` builder. Benches and
  examples keep their two inline copies each: they are separate compilation
  units that cannot reach `pub(crate)` helpers, and exporting the policy for
  demo code would widen the public API for no consumer.
- **flui-engine: `render_scene_content` borrows the painter in place.** The
  `self.painter.take()` / reassign dance — an enabler left over from the
  `Arc<Mutex<OffscreenRenderer>>` removal, tracked as the blocker-free entry
  on ARCHITECTURE.md's Outstanding-refactors list — is gone; the `Backend`
  holds disjoint `painter`/`offscreen` field borrows for the frame.
  ARCHITECTURE.md was reconciled against the code while landing this: the
  per-frame `Arc::clone` entry had already been resolved by deletion
  (`RenderContext` lost its device/queue fields), the `offscreen.rs` split had
  already landed as `offscreen/{mod,blit,blur,mask}.rs`, and port-check
  trigger 5's whitelist comments now describe the current shape instead of
  line numbers that no longer exist. The `Arc<Mutex<TexturePoolInner>>`
  refactor stays open on the list — it re-plumbs ownership through the
  painter/offscreen hot paths and needs GPU-verified behavior, not just a
  clean compile.
- **Toolchain 1.97.1 → 1.98.0 (development pin only; MSRV floor stays 1.97).**
  `rust-toolchain.toml` is the *development* toolchain and moves independently
  of the floor, which remains a separate promise checked by the one `msrv` job —
  nothing in 1.98 is used that 1.97 cannot compile, so the floor was not moved.
  Three new lints fired under the `-D warnings` gate and were fixed rather than
  suppressed: `clippy::manual_midpoint` (`Alignment::along_size`, and the
  superellipse clip reduction in `flui-engine`'s instancing — `f32::midpoint`
  now expresses what `0.5 * (a + b)` meant), `clippy::chunks_exact_to_as_chunks`
  (eight per-pixel `chunks_exact(4)` walks, now `as_chunks::<4>()`, which hands
  the loop a `&[u8; 4]` instead of an unsized slice), and `clippy::drain_collect`
  (three `drain(..).collect()` hand-offs, now `mem::take`, which moves the
  existing allocation instead of copying it into a fresh one). A fourth,
  `clippy::unused_async_trait_impl`, is allowed workspace-wide with its
  rationale in `Cargo.toml`: its fix desugars one impl of an `async fn` trait to
  `-> impl Future` while the trait's other impls stay `async fn`, and it has no
  answer for the feature-gated stub pairs whose `async` is exactly what keeps
  the two configurations' signatures identical.
- **GPU stack: wgpu 29 → 30, and the five crates pinned to it.** `naga`/`naga_oil`
  0.22 → 0.23, `wgsl_bindgen` 0.22 → 0.23, `wgpu-profiler` 0.27 → 0.28,
  `glyphon` 0.11 → 0.12, `cosmic-text` 0.18 → 0.19 — they move as one set
  because each pins the others' majors. Four API changes reach this tree:
  `SurfaceConfiguration` gained `color_space` (set to `Auto`, which is wgpu's
  own pre-30 behaviour — naming a wide-gamut or HDR space would change how the
  shaders must encode their output and is a rendering decision, not a version
  bump); `RequestAdapterOptions` gained `apply_limit_buckets` (set to `false`:
  the bucketing is an anti-fingerprinting measure for embedders exposing a GPU
  to untrusted content, and rounds real adapter limits down to a coarse tier);
  `SurfaceTexture::present()` moved to `Queue::present(texture)`; and
  `BufferSlice::get_mapped_range()` now returns a `Result`. `VertexState::buffers`
  became `&[Option<VertexBufferLayout>]`, so every layout is `Some`-wrapped —
  `None` would mean a deliberately empty slot, which no pipeline here has.
  cosmic-text 0.19 made the `Buffer` setters lazy: `set_size`/`set_text`/
  `set_rich_text` no longer take `&mut FontSystem` (shaping still does, at
  `shape_until_scroll`), which touches both flui-painting's text layout and
  flui-engine's glyph cache.

  Two stale claims in comments were corrected rather than renumbered, because
  checking them showed the underlying facts had changed: `wgsl_bindgen` 0.23.3
  no longer emits the `#![allow(...)]` inner attribute that `build.rs` strips
  (verified against the generated output — the strip is now a guard, not a
  fixup), and `wgpu-profiler` 0.28 *does* type-check for `wasm32-unknown-unknown`
  (measured with the guard lifted), so the `compile_error!` rejecting
  `gpu-profiler` on wasm now stands on "unexercised here", not "impossible".
  The `RUSTSEC-2026-0253` ignore in `deny.toml` was re-derived against glyphon
  0.12.0's own source, not carried over: both grounds still hold and 0.12 still
  caps `lru` at `^0.16.2`.

  Not verified locally: the GPU readback and deterministic-replay suites
  (`testing`) compile clean but cannot execute here — this container
  has no Vulkan ICD and no `/dev/dri`, the same reason CI's Linux jobs don't run
  them. Their executing coverage is CI's `gpu-test` job on WARP.
- **Dependency refresh: full `cargo update` plus ten semver-major bumps.**
  `reqwest` 0.12 → 0.13 (its `rustls-tls` feature is now spelled `rustls`;
  0.13 also makes rustls the default backend, so `default-features = false`
  plus the explicit backend is what keeps the openssl ban enforced rather than
  merely defaulted — and the `rustls` feature's provider is aws-lc-rs, a native
  build that adds a cmake/C-toolchain requirement to `flui-assets`' `network`
  feature; reqwest 0.13 offers no ring-backed alternative short of
  `rustls-no-provider`, which would push crypto-provider installation onto every
  consumer), `syn` 2 → 3, `pollster` 0.4 → 1.0, `criterion` 0.7 → 0.8,
  `cliclack` 0.3 → 0.5, `indicatif` 0.17 → 0.18, `serial_test` 3 → 4,
  `notify-debouncer-mini` 0.5 → 0.7, `tower-http` 0.6 → 0.7, `x11rb` 0.13 → 0.14.
  None of them needed a source change. `flui-app` also stopped carrying its own
  `pollster = "0.4"` pin and now takes the workspace one, which is what had been
  holding a second copy of the crate in the lockfile.
- **`flui-scheduler`'s `Scheduler` hard-renamed `UpdateScheduler`, and
  reshaped around a deadline-bounded Idle slice** (#556): `WeakScheduler` →
  `WeakUpdateScheduler` too, no alias, workspace-wide (61 files outside
  `flui-scheduler` itself). `drive_frame` gained a `deadline: IdleDeadline`
  parameter (a newtype over `Instant`, closing off a same-typed-parameter
  swap hazard) that bounds `Priority::Idle` task execution only —
  `Priority::Animation`/`Build` always run, and a panicking task can no
  longer leak a stale deadline into a later frame. `UpdateScheduler` now
  carries no `frame_duration`/`target_fps` field or accessor, no fixed
  `FrameDuration::FPS_60` default, and no `FrameSkipPolicy`/
  `SchedulingStrategy` machinery (all dead surface with zero production
  consumers); the `budget()` `MutexGuard` accessor is replaced by an owned-
  value `budget_snapshot()`. The `VsyncScheduler` fixed-rate vsync simulator
  family is deleted outright (zero production consumers; real pacing lives
  in the blocking Fifo present, ADR-0029). See `crates/flui-scheduler/
  CHANGELOG.md` for the full per-symbol breakdown.
- **Four miri-confirmed UB paths closed in the Android page-aligned
  allocator** (#584): `Drop` recomputed the dealloc layout as
  `capacity * size_of::<T>()`, which undershoots the real page-rounded
  allocation whenever `size_of::<T>()` doesn't evenly divide it — now stores
  the allocated byte size verbatim and reuses it. Zero-capacity construction
  and zero-sized `T` both reached the global allocator with a zero-size
  `Layout` (the former now uses a non-null dangling pointer instead of
  allocating; the latter is now a compile-time assertion). Separately,
  `alloc_page_aligned`'s rounding arithmetic could wrap to zero under
  release's `overflow-checks = false`, reaching the allocator with a
  zero-size `Layout` for a large enough request — now checked, returning
  `Err` instead of wrapping. All four verified by replicating the auditor's
  scratch-crate `cargo +nightly miri test` harness: reproduces the original
  UB before the fix, clean after.
- **Toolchain 1.96.1 → 1.97.1, MSRV 1.96 → 1.97.** `rust-toolchain.toml` no
  longer mirrors the MSRV: it is now explicitly the *development* toolchain
  (a pin at the floor hides new lints and codegen changes from the developer
  until CI surfaces them), while the floor stays a separate promise checked by
  the one `msrv` job. Three 1.97 stabilizations earn the bump: **v0 symbol
  mangling by default** (the release binary now carries 5110 v0 symbols and
  zero legacy — generic frames demangle with real type parameters instead of
  an opaque hash), **`build.warnings`** (below), and the integer
  bit-manipulation APIs (`bit_width`, `isolate_lowest_one`,
  `isolate_highest_one`, `lowest_one`, `highest_one`) that the render-node
  dirty-flag bitset is a candidate for. One new pedantic lint
  (`clippy::manual_assert_eq`) fired at two sites and was fixed.
- **CI warnings gate: `RUSTFLAGS=-D warnings` → `CARGO_BUILD_WARNINGS=deny`.**
  `RUSTFLAGS` is part of the rustc fingerprint, so the miri job — which needs
  a different value — got a disjoint `target/` cache and had to blank the flag
  wholesale, losing every other check with it. The Cargo knob is applied after
  compilation: measured, toggling it recompiles nothing while switching
  `RUSTFLAGS` recompiles. miri now sets `CARGO_BUILD_WARNINGS=warn` and shares
  the cache.
- **Release profile: `strip = "symbols"` → `strip = "debuginfo"`.** Stripping
  the symbol table left release builds unprofilable and crash reports
  unsymbolicated — `perf`, flamegraph, samply, Tracy and minidumps all resolve
  frames through it. Cost measured on `target/release/flui`: 3 437 096 →
  4 394 832 bytes (+935 KiB, +27.9%); DWARF is still dropped.
- **Performance overlay wired to `AppConfig`.** `show_performance_overlay` was
  write-only: the builder set it and nothing read it, while the layer, the
  rolling stats window and a real wgpu draw path all already existed. The chain
  is joined in `draw_frame` phase 4. Scope: it reports FPS and average frame
  time only — the renderer ignores the frame counter and the option mask — and
  the sampled interval is between *composited* frames, so it is a repaint rate.
- **`Defunct` is now an absorbing lifecycle state.** `Lifecycle::can_activate` /
  `can_deactivate` existed but were called only from tests; every mutator
  assigned unconditionally, so `Defunct → Active` was reachable through the
  public `ElementCore::activate` and would revive an element whose state was
  disposed. Both predicates are now asserted (debug-only) in `ElementCore` and
  in the hand-rolled `RootRenderElement`/`ErrorElement`.
- **`TextRange` consolidated into flui-types**, whose copy was already a strict
  superset; the canonical type gains `Clone, Copy, PartialEq, Eq, Hash`. Under
  0.x this is a breaking change (`cargo semver-checks`: `copy_impl_added` +
  `struct_missing`) — 0.2.0 → 0.3.0 when published.
- **`once_cell` dropped as a direct dependency** in favour of
  `std::sync::{OnceLock, LazyLock}`, which most crates already used. It remains
  in the lockfile transitively via `ahash` ← `hashbrown` ← `dashmap`.
- **Workspace lints:** `unexpected_cfgs`, `unsafe_op_in_unsafe_fn` and
  `unused_must_use` at `deny`, each measured at zero sites first so they are a
  regression bar rather than a migration. `clippy::undocumented_unsafe_blocks`
  was tried and reverted — see the note in `Cargo.toml`: it only sees what the
  Linux job compiles (~91 further sites live in the Windows/macOS backends), and
  enabling it before auditing produced comments that stated invariants the code
  does not establish.
- **`just test-release` went from 4 red suites to 1.** The recipe now excludes
  flui-platform, matching the CI `test` job — that crate's suite is red
  independently of the profile (the STATUS_HEAP_CORRUPTION investigation), so
  including it made the recipe permanently red. With that scoped, eleven
  `#[should_panic]`-over-`debug_assert!` tests across eight files could not pass
  in release, where the assertion does not exist; they are now
  `cfg(debug_assertions)`-gated. Two were introduced by this branch, nine
  predated it.
  One suite is still red and is NOT fixed here: flui-interaction's
  `eager_dispose_clears_state` has a deliberate release-only branch asserting
  that a post-`dispose` `add_pointer` does not reach the arena, and it does.
  Verified red on `main` independently of this branch — a real defect in the
  recognizer's dispose guard, not a profile artifact.

- **`wasm-check` now passes.** The job had never been green: 11 errors across
  `flui-scheduler` (2), `flui-platform`'s web backend (4) and `flui-app` (5).
  The `flui-app` five are not dead code — the job runs `cargo check` without
  `--all-targets`, so the desktop runner (`cfg(not(target_arch = "wasm32"))`)
  and the tests that consume them are absent from the wasm lib check; they
  carry `#[cfg_attr(target_arch = "wasm32", allow(dead_code))]` naming the
  consumer rather than a blanket allow.
- Lockfile: `wgpu` 29.0.3 → 29.0.4, `anyhow` → 1.0.103, `crossbeam-epoch`
  → 0.9.20, `swash` → 0.2.9 (off a yanked version). `clippy.toml` gains
  an `msrv` key so MSRV-aware lints track the declared floor.
- **One integration-test binary per heavy crate**: flui-widgets (49 → 1 +
  the pre-existing `parity` target), flui-rendering (36 → 1), flui-view
  (24 → 1) — each root `tests/*.rs` used to statically link the whole wgpu
  stack into its own binary (~5.9 GB across 188 executables). Files stay in
  place as `#[path]` modules; test parity proven exactly (550/782/252 tests
  unchanged). Bevy-style `dynamic_linking` (`flui-dylib`) noted as the next
  lever if needed.
- **Dev profile `debug = 1` → `"line-tables-only"`**: panic backtraces keep
  file:line, the variable/scope DWARF that ballooned `target/debug/deps`
  toward ~20 GB is gone, and the local default now matches what CI has built
  with since PRs #236/#242. `--profile dbg` remains the full-debuginfo
  opt-in. New `just sweep` recipe (cargo-sweep) prunes stale artifacts.

- **Lint normalization**: every workspace crate now inherits
  `[workspace.lints]` via `[lints] workspace = true` (12 crates previously
  bypassed workspace lints entirely; 3 carried stale local copies), enforced
  by a new drift guard in `scripts/check-workspace-inventory.sh`.
- `flui-assets` restored to `[workspace] members` — it is built and tested by
  CI again.

### Removed

- **`flui_scheduler::{UpdateScheduler::schedule_frame, FrameCallback}`**
  (#1058): the legacy, `&FrameTiming`-argument frame-callback registration
  is deleted, no alias. Its dispatch loop held the `current_frame` mutex
  across every callback invocation, so a registered callback that read
  `current_frame()` deadlocked on itself. `RenderingFlutterBinding::request_visual_update`
  (its only production caller) now routes through
  `UpdateScheduler::ensure_visual_update()` instead, which also fixes a
  second bug: the retired call ignored `frames_enabled` and scheduled a
  frame even while the app was backgrounded. Sibling lock-then-drop sites
  fixed in the same change (tracked with #1150): `cancel_frame_callback`,
  `remove_lifecycle_state_listener`, and `remove_timings_callback` no
  longer drop a removed callback while its own collection is still locked.
- **`paint_alpha`, `paint_layer_blend`, `paint_transform`** (#996): the three
  separate hooks on `RenderBox`/`RenderSliver`/`RenderObject`, and
  `RenderNode`'s dispatch of them, are gone — replaced by the single
  `paint_effects` value described above. `ClipGeometry::with_clip_scope` is
  replaced by `to_paint_clip`. `flui_widgets::testing::LaidOut::opacity_paint_alpha`
  and `sliver_opacity_paint_alpha` keep their names (both still read an alpha
  out of a test probe) but now read it off the `paint_effects` value instead
  of the deleted hook.

- **Five dead stubs leave the public `Platform` trait** (#551, executing
  ADR-0039 §2's recorded "deleted outright in slice 3, not moved" decision):
  `hide`, `hide_other_apps`, `unhide_other_apps`, `should_auto_hide_scrollbars`,
  and `window_stack`. The first four were default bodies with no backend
  override and no caller anywhere in the workspace. `window_stack` had three
  impls and still no caller: winit returned `None` ("not easily supported"),
  macOS/Windows/Android inherited the default, and only the headless double and
  the web backend returned toy values — a surface whose only implementations
  were the ones that could not fail. ADR-0034's "no surface ahead of a real
  implementation *and* consumer" says these die rather than ride along; each
  returns, on `OwnerPlatform` if owner-affine, with its first real
  implementation and consumer. This is the ungated half of slice 3: moving the
  eight owner-affine methods off the trait stays blocked on the Android
  on-device validation ADR-0039 makes its precondition.

- **Singleton retirement, completed in six PRs (#586–#593).** `flui-app`'s
  process-global service host — `AppBinding`, plus its `WidgetsFlutterBinding`
  alias — is deleted outright, not deprecated, along with
  `flui-foundation`'s `impl_binding_singleton!` macro and the
  `HasInstance`/`BindingBase` trait pair, `SemanticsBinding`, and the
  `PaintingBinding`/`Scheduler` singleton `::instance()` accessors. Ownership
  moves to where it should have lived all along: `AppRuntime` (loop-scoped:
  wake, clipboard), `UiRealm`/`PresentationState` (realm/presentation-scoped:
  renderer, vsync, frame counters, haptics, and semantics fan-out through a
  new per-presentation `SemanticsHost`), and a `WeakScheduler`-backed
  `Scheduler` owned fresh per realm — the weak handle breaks the
  `scheduler → transient queue → ticker closure → scheduler` `Arc` cycle
  that used to leak every active ticker for the scheduler's whole lifetime.
  `UiRealm`'s transitional at-most-one-construction guard
  (`REALM_CLAIMED`, `UiRealmError::AlreadyExists`) — which existed only
  because `UiRealm` used to front that process-global state — is deleted
  too; four new tests prove actual realm coexistence (independent mounts,
  scheduler phases, and gesture arenas, including across two threads and
  across a `GlobalKey` collision in two different realms) rather than
  merely the absence of the deleted guard. The test locks this family
  needed — `SINGLETON_WINDOW_TEST_LOCK`, `SCHEDULER_PHASE_TEST_LOCK`,
  `SEMANTICS_TEST_LOCK` — are gone along with the state they serialized;
  each is now a `forbidden_pattern` ratchet in `docs/runtime-contract.toml`
  so a PR cannot reintroduce the pattern by reintroducing the name.

  **Breaking (pre-1.0, sanctioned):** `flui_app::{AppBinding,
  WidgetsFlutterBinding}` and the `flui::app` facade re-export of both are
  gone; `crates/flui-foundation/src/binding.rs` (`BindingBase`,
  `HasInstance`, `impl_binding_singleton!`) is deleted wholesale;
  `SemanticsBinding` is deleted, not slimmed. `run_app`/
  `run_app_with_config`/`run_direct` signatures are unchanged — this is an
  internal-ownership change, not a public entry-point break. Recorded here
  as the semver trigger for a `0.2.0` → `0.3.0` bump once this workspace is
  published; no git tag is cut by this entry (release-lead's call, separate
  from this changelog sweep).

### Fixed

- **`flui-devtools`' frame profiler never saw a frame** (`flui-devtools`,
  `flui-scheduler`). `FrameTimingLayer` delimited frames by a span named
  `render_frame_entered` that no crate opened, so in a real app the profiler
  recorded nothing while its unit tests, which emitted that span themselves,
  stayed green. `UpdateScheduler::drive_frame`, the one frame driver every
  runner and `HeadlessBinding::pump_frame` share, now opens a `DEBUG` span
  named `frame` around the whole frame, the layer listens for it, and a new
  end-to-end test drives a real tree through the headless binding and reads
  the profile back. The layer now also carries its own per-layer filter:
  FLUI's default `INFO` log filter no longer starves it, and attaching it no
  longer declares interest in every callsite in the process.
- **`flui-cli` did not compile on Windows** (`flui-cli`, `flui-build`): the
  Ctrl-C listener's runtime asked for `enable_io`, which tokio's `signal`
  feature only exposes on Unix; it now uses `enable_all`. Under the
  workspace's `-D warnings` the Windows build then tripped on code that only
  macOS or Unix reaches (simctl parsing, iOS targets, hot-key bindings,
  `flui-build`'s bundle-staging `Path` import); each is now gated to the
  platform that uses it. The `cross-typecheck` CI job gains a Windows clippy
  step for the CLI so this class cannot return.
- **`flui devices` hung forever on macOS** (`flui-cli`): browser detection
  ran `Safari -v`, which launches Safari instead of printing a version.
  Versions are now read from each app's `Info.plist`, and every external
  probe the CLI makes (`adb`, `xcrun`, `rustup`, `java`, …) runs with a
  10-second deadline through `flui_cli::proc`.
- **`flui doctor` reported Apple's Java stub as installed and exited 0 after
  "Some checks failed"** (`flui-cli`): the check now requires the probe to
  exit successfully, and a failed required check exits 3. The always-green
  "wgpu: Available" line is gone.
- **Desktop logs carried ANSI escape codes into pipes** (`flui-log`): the
  compact formatter coloured unconditionally, so `flui run --json` forwarded
  log lines full of `\u001b[…` sequences. Colour now follows `NO_COLOR`,
  `CLICOLOR_FORCE`, and whether the stream is a terminal.
- **`flui upgrade` installed a crate that does not exist** (`flui-cli`): it
  asked Cargo for `flui_cli`; the package is `flui-cli`, and a source
  install now gets the matching `cargo install --path` hint.

- **Nothing rendered in a browser** (`flui-engine`): every clip-capable
  pipeline failed to compile under WebGPU because two shaders took
  screen-space derivatives in non-uniform control flow — `clipAlpha` called
  `sdfToAlpha` inside a branch on the per-instance clip kind, and the arc
  shader took its angular gradient after a per-instance early return. Tint
  (every browser) rejects that; native naga accepted it, which is why the
  defect never showed on desktop. Both shaders now compute derivatives
  unconditionally and `select` afterwards. `scripts/check-wgsl-uniformity.py`
  (in `just gate` and CI) refuses the shape structurally, since no host-side
  validator catches it: it walks tokens rather than lines, so `} else {`,
  a one-line `if c { return x; }` and a split `for` header are all seen;
  `textureSample` and its bias/compare forms count as derivatives (Tint
  applies the same rule to them); a branch on a uniform value is admitted
  only with a `wgsl-uniformity: uniform` comment saying why (the blur and
  morphology loops); and `--self-test` pins every one of those layouts.
  `examples/web_counter` and `just web-counter-build` are the runnable
  browser evidence.
- **`flui create` initialised a git repository in the caller's working
  directory** (`flui-cli`): `git init` ran without a directory, so the
  repository landed wherever the command was run from rather than in the
  generated project. It now runs inside the project; a regression test runs
  the CLI from an unrelated directory and checks both locations. (A stray
  empty `.git` this left inside `crates/flui-cli` during a test run also made
  `cargo package` refuse every file in that crate as uncommitted.)

- **Ticker callbacks survive a reentrant restart, mute, or panic** (#1059):
  `flui-scheduler`'s `Ticker` restored a checked-out callback whenever the
  ticker was active, without asking whether that run was still the one that
  checked it out. Chaining the next animation from a status listener — the
  canonical idiom — therefore left two live tick chains on one controller
  (ticking and notifying twice per frame while a run stayed in flight, only one
  of them cancellable), muting
  from inside a tick discarded the callback that `mute()` promises to retain,
  and a panicking callback emptied the slot for good. The slot is now a
  three-state machine leased across the callback by an RAII guard, with one
  predicate gating every scheduling site.

- **Scheduler frame state closes before a pre-pipeline panic propagates** (#1057):
  `flui-scheduler`'s `drive_frame`/`drive_frame_with_lane` used to catch only
  a panicking pipeline; a panic from a transient callback, the mid-frame
  async-driver poll, a persistent callback, or a priority task — every one
  of which runs before the pipeline slot opens — escaped uncaught and left
  the scheduler's phase, `frame_scheduled` latch, and frame-completion
  waiters stuck. One recovery boundary now covers the whole frame lifetime
  `drive_frame` owns, `execute_frame`/`execute_frame_with_lane` share it
  instead of a second unguarded sequence, every queue drained before the
  pipeline runs preserves a panicking entry's still-queued siblings, and a
  panicking async future no longer leaves a zombie task slot behind. See
  `crates/flui-scheduler/CHANGELOG.md` and `crates/flui-scheduler/ARCHITECTURE.md`.
  A follow-up review round found and fixed two regressions the bound itself
  introduced (a reentrant task's own buffer dropped on a later task's panic;
  concurrent transient-callback registration racing id assignment against
  queue order) — see the crate changelog.
- **Held pointer terminal replay after an active `Down`** (#561): pointer
  `Move`/`Up`/`Cancel` events that arrive during an uncommitted-frame window
  now queue even when their matching `Down` was already dispatched before the
  failed frame. The replayed terminal event releases the cached gesture route
  instead of leaving recognizers stuck pressed.
- **Presentation-local retry and frame-commit accounting** (#561): a failed
  presentation retains its last submitted scene, cannot have its retry
  cancelled by a clean sibling later in the same pump, and remains
  uncommitted until a painted frame is accepted as `Presented` or `NoPresent`.
  Panics after pipeline work re-dirty only the failed presentation so the
  automatic retry can repaint, while stationary-pointer re-hit-testing remains
  available to a separately committed primary presentation.
- **A programmatic `PlatformWindow::close()` on the winit backend now actually closes the
  window** (#919). It used to hide the window and fire `on_close` but never leave the backend's
  tracking map, against which the exit policy is consulted, so an application closing its own
  last window lingered forever with nothing on screen. Both close routes now run one teardown
  body on the event-loop owner's next turn; `on_close` therefore fires on the owner thread
  whatever thread called `close()`, never synchronously inside the call. `WinitWindow` moved
  from `flui_platform::traits` to `flui_platform::platforms::winit`, and its constructor is
  backend-private (it now needs the owner lane). The live-smoke harnesses gained a
  `FLUI_SELF_CLOSE_ROUTE=programmatic` cycle that drives this route through the real app.

### Pre-changelog milestones

Recorded retroactively from `docs/ROADMAP-TRACKER.md`; evidence links live
there.

- **2026-07-01 — Core.2 exit**: full render-object catalog (37 concrete
  RenderBox/RenderSliver objects extracted to `flui-objects`), 250/250
  per-object harness tests, catalog CI guard.
- **2026-06-30 — Core.0 exit / Core.1 substantially delivered**: view/element
  core contracts locked (`specs/004-view-element-core`, keyed reconciliation,
  `IntoView` authoring surface, element storage); `flui-widgets` slice with 14
  widget families; `flui-animation` re-enabled; production vsync + lazy
  slivers end-to-end; C1.11 contract-validation report (4,847 tests passing).
  Core.1 formal exit still awaits a windowed run (C1.10/C1.12 — see the OPEN
  ITEM in the tracker).
- **2026-06 — GPU engine hardening**: WGSL readback/oracle suite (~440 tests)
  runs on CI via WARP; image-filter pipeline (blur/ColorFilter) sized to
  content bounds; deterministic-replay IR purity witness.
- **Business.1 (in flight)**: Flutter widget-catalog port continues
  (`RichText`/`Icon` landed); tracked in `docs/ROADMAP.md`.

[Unreleased]: https://github.com/vanyastaff/flui/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/vanyastaff/flui/releases/tag/v0.1.0
