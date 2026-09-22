# FLUI Developer Experience Audit — CLI, Templates, Hot Reload, DevTools, First-Run

Date: 2026-09-22. Repo: /Users/vanyastafford/Develop/flui (main, fbed9408).

## 1. CLI (`crates/flui-cli`) vs Flutter / dx / cargo-leptos / trunk

Subcommands found in `crates/flui-cli/src/main.rs` (clap `Commands` enum) and
`crates/flui-cli/src/commands/`:

`create`, `run`, `build`, `test`, `analyze`, `doctor`, `devices`, `emulators`
(list/launch), `clean`, `upgrade`, `platform` (add/remove/list), `format`,
`devtools`, `completions`.

This is a *broad* surface — broader than most Rust UI-framework CLIs today
(Dioxus `dx` and `cargo-leptos` do not ship `doctor`, `devices`, or
`emulators`). Coverage vs Flutter's list:

| Flutter | FLUI | Notes |
|---|---|---|
| `flutter create` | `flui create` | present, richer (7 templates, `--hot-reload`, `--local`, `--lib`, `--dry-run`, `--no-check`, interactive mode) |
| `flutter run` | `flui run` | present; drives hot reload, `--scene` fast path for Android |
| `flutter build` | `flui build` | present; per-platform flags (`--split-per-abi`, `--optimize-wasm`, `--universal`) — 903 lines, the largest command file |
| `flutter test` | `flui test` | present but thin (52 lines): wraps `cargo test` with filter/`--unit`/`--integration`; `--platform` flag is parsed but **not implemented** (`_platform` in the signature) |
| `flutter doctor` | `flui doctor` | present (399 lines) |
| `flutter devices` | `flui devices` | present (401 lines), plus `flui emulators` (617 lines) which Flutter folds into `devices`/`emulators` too |
| `flutter pub` | *(none)* | no package-manager subcommand; relies on `cargo add`/`cargo update` directly. `flui upgrade` covers self-update + `cargo update`-style dependency bumps, but there is no `flui pub get`/`pub add` equivalent — acceptable since Cargo already owns this job, but `flui upgrade` (53 lines) is a stub relative to its name |
| `flutter analyze` | `flui analyze` | present, thin (40 lines): literally `cargo clippy --workspace -D warnings` (+`--pedantic`, `--fix`). No FLUI-specific lints (e.g. widget-tree conventions, missing `impl_render_view!`, etc.) |
| `flutter clean` | `flui clean` | present |

Net: the **subcommand inventory is essentially complete** relative to Flutter.
The gap is depth, not breadth: `test`, `analyze`, and `upgrade` are thin
wrappers around `cargo test` / `cargo clippy` / `cargo update` with no
FLUI-aware behavior (no per-widget golden-test runner, no framework-specific
lints, no dependency-compat matrix). `devtools` (see §5) is the one command
that is honest about doing nothing yet. Compared to `dx serve`/`dx bundle`
(single dev-loop command) FLUI's CLI is more Flutter-shaped (separate
verbs) which is arguably the right call for a framework this wide
(desktop/mobile/web/Android NDK).

## 2. Templates (`crates/flui-cli/src/templates/`)

Templates: `basic`, `counter` (default), `todo`, `dashboard`, `widget`,
`plugin`, `empty` (declared in `main.rs`'s `Template` enum); implementation
files present are `basic.rs`, `counter.rs`, `hot_reload.rs`, plus a
plan-based generator in `mod.rs` (`TemplateBuilder::plan()` /
`::generate()`) that actually drives `create`. `commands/create.rs` calls
`templates::TemplateBuilder`, which for the counter template ultimately
invokes `counter::generate(name, org, &source, &platforms)`.

**Dependency source** (`DependencySource`, resolved in `create.rs`):
- Default (no `--local`): pins `flui = { git = "https://github.com/vanyastaff/flui", tag = "v0.1.0" }` (and a `dev-dependencies` copy with `features = ["testing"]`). The tag **exists** — verified with `git ls-remote --tags` (`v0.1.0` → `dc479269`).
- `--local`: path deps into `../../crates/...` — only works from inside the FLUI source tree.
- FLUI is **not on crates.io** (confirmed by comments in the CLI source itself, e.g. "FLUI is not yet published to crates.io... version strings will not resolve").

Note: an older code path in `templates/counter.rs` still contains a
`generate_cargo_toml` that emits **unversioned crates.io dependency strings**
(`flui-app = "{version}"`, etc.) for the non-local case — this is dead/stale
relative to the git-tag path the CLI actually exercises today (confirmed:
`mod.rs` calls the git-tag generator; the version-string function was not
reached in the live `flui create` run). This is a discoverable landmine for
anyone reading `counter.rs` top-to-bottom expecting it to reflect current
behavior — worth cleaning up or deleting.

**What a user gets out of the box** (verified by running `flui create`, see
§3): a real two-widget app (`CounterApp` → `Theme` → `SafeArea` →
`CounterView`, a `StatefulView` with `StateCell<usize>` and an
`ElevatedButton` incrementing on press), a `flui.toml` (app metadata, target
platforms, asset dirs), a `README.md` with `flui run`/`flui build`/`flui
test` instructions, and a **real widget test** using
`flui::testing::widgets::{lay_out, tight}` that pumps pointer input,
asserts label text changes, and asserts state survives a parent rebuild.
This is materially better than the typical "hello world" scaffold — the
generated test is a legitimate `WidgetTester`-style example (see §6).

`rust-version = "1.97"` in the generated `Cargo.toml` matches the installed
toolchain (`rustc 1.98.1`), so no MSRV friction there.

## 3. First-run flow — RESULT: works, but only after a real environment bug

Executed in the scratch dir (all cargo invocations timed):

1. **`cargo build -p flui-cli`** inside the FLUI workspace **failed
   immediately** with:
   ```
   error: linking with `cc` failed: exit status: 1
   clang: error: invalid linker name in argument '-fuse-ld=lld'
   ```
   Root cause: `/Users/vanyastafford/Develop/flui/.cargo/config.toml`
   unconditionally sets `rustflags = ["-C", "link-arg=-fuse-ld=lld"]` for
   both `x86_64-apple-darwin` and `aarch64-apple-darwin`, but `lld` is
   **not installed** on this stock macOS machine (`which lld` /
   `ld.lld` → not found; no `llvm`/`lld` keg in Homebrew). This is a
   **first command in `docs/getting-started.md`** (`cargo build
   --workspace`) failure mode that the doc's own troubleshooting table does
   not mention (it lists `link.exe` on Windows but nothing about `lld` on
   macOS). **Any contributor on a clean Mac without `brew install llvm` (or
   equivalent) hits this on the very first build**, with a cryptic clang
   error, not an FLUI-authored message.
   - Workaround used to continue this audit: `cargo build -p flui-cli
     --config 'target.aarch64-apple-darwin.rustflags=[]'`. Built cleanly in
     **31.7s** (399% CPU, 8 jobs).
2. **`flui create hello --no-check`** (from the built binary, output
   directed at the scratch dir): instant (<1s), clean `cliclack`-styled
   output, correct next-steps hint (`cd hello && flui run`).
3. **`cargo check`** inside the generated `hello/` project (git-tag
   dependency, cold — first fetch/build of the whole FLUI dependency graph
   from GitHub): **74.5s wall** (245s user / 32s sys, 373% CPU), ~140
   crates compiled/checked (objc2-*, wgpu 30.0.1, cosmic-text, lyon, etc.),
   **zero errors**. Crucially, this succeeded **without** the lld
   workaround — the generated project is a standalone `[workspace]` outside
   the FLUI repo tree, so it never inherits the parent repo's
   `.cargo/config.toml`. **The lld bug only bites contributors building the
   FLUI repo itself, not end users of `flui create`.**

**Verdict: first-run works end-to-end for an app developer** (`flui create`
→ `cargo check` succeeds, ~75s cold, real git dependency resolves). The
**contributor** first-run (`cargo build --workspace` per
`getting-started.md`) is **broken out of the box on macOS** unless `lld` is
already on `PATH` — a significant, easily-hit rough edge for exactly the
audience `getting-started.md` targets (new contributors), even though it
doesn't affect the packaged CLI's own template-generation path.

## 4. Hot reload (`docs/hot-reload.md`, `crates/flui-hot-reload/`)

Two-layer model, clearly documented:
- **Layer 1** (build orchestration, dev-time): `SourceWatcher` → `cargo
  build` → artifact on disk. Lives in `flui-hot-reload`'s `source-watch`
  feature + `flui-cli` + `flui-devtools`.
- **Layer 2** (artifact reload, runtime, native only): `HotReloadDriver` →
  mtime poll → `dlopen` reload → new `Scene`/build. Always on for non-wasm
  targets.

Reload strategies (`ReloadStrategy` in `flui-hot-reload/src/strategy.rs`):
`ProcessRestart` (default `flui run`: full rebuild + kill/respawn),
`PluginDylib` (manual/`cargo watch` + `HotReloadDriver::poll()`, host stays
alive), `BuildAndDeploy` (Android `flui run --scene`: ndk build + `adb
push`, host polls mtime on-device), `None` (`--release` / WASM).

**State preservation**: real hot reload (state kept, `build()` re-run) is
claimed for `flui run`'s worker-dylib swap path, gated behind `flui create
--hot-reload` which scaffolds a **three-crate workspace**
(`{name}-types` + `{name}-logic` + `{name}-host`) — state must live in the
host binary's Element tree, never the reloadable `.so`, per the doc's
"critical insight." Editing `-logic` triggers an in-process worker reload
with `State` preserved; editing `-types` forces a full process restart
(shared-type layout / `TYPE_FINGERPRINT` changed). The default single-crate
template (no `--hot-reload`) only gets `ProcessRestart` — i.e., **the
default `flui create` project does NOT get Flutter-parity state-preserving
hot reload**; you must opt in with a flag that also changes your project's
crate topology.

Authoring contract: no special macros beyond the framework's own
`#[derive(StatelessView/StatefulView)]`; the worker exposes
`scene_plugin!(fn)` / `app_plugin!(Widget)` FFI entry points
(`flui_scene_build` / `flui_app_build`). Both worker and host **must** be
built in one `cargo build -p worker -p host` invocation — documented as a
hard correctness requirement, not a convenience, because per-invocation
Cargo feature unification is what keeps `TypeId` stable across the two
binaries; building them separately silently produces two `TypeId`s for the
same type and a first-frame panic. This is a subtle, sharp edge that is at
least well-documented.

**Comparison**:
- **Flutter hot reload**: single mechanism, works uniformly across the app,
  no user-visible crate split, near-instant (<1s typical), state preserved
  by design (VM-level).
- **Dioxus `subsecond`**: similar dylib-hot-patch idea (patch running
  process in place) but does not require a 3-crate workspace split — it
  patches functions in the same binary.
- **FLUI**: closer to `hot-lib-reloader`'s pattern (explicitly cited as the
  model for content-addressed staged dylib paths, `{stem}-hot-{hash}{ext}`,
  to dodge macOS dyld's deferred-unmap-serves-stale-image behavior). Real
  engineering effort here (mtime-based background watcher thread so an idle
  window still notices artifact changes; ad-hoc codesign of staged
  dylibs), but the **three-crate opt-in requirement** is materially more
  friction than Flutter or Dioxus for a developer who just wants "edit,
  save, see it update."

## 5. DevTools (`crates/flui-devtools/`) — issue #1125 is correct

`flui devtools` (`crates/flui-cli/src/commands/devtools.rs`) is **explicitly
self-reporting as not implemented**: regardless of whether the CLI is built
with the `devtools` feature, running it prints "DevTools server is not
implemented — no listener opens on port {port}" (or, feature off, "DevTools
is not available in this build") and returns a non-zero exit
(`CliError::not_implemented`). There is **no server, no GUI, no port ever
opened** — confirmed by reading the command source, which the code comments
say plainly ("Neither build reaches a running server").

What actually exists in `flui-devtools` (a **library only**, README +
FEATURES.md read in full):
- `profiler` + `frame_timing_layer` (feature `profiling`): per-frame
  build/layout/paint/compositing timings via a `tracing::Layer`
  subscribing to the framework's own spans; jank detection, FPS.
- `timeline` (feature `timeline`): Chrome-trace (`chrome://tracing`) +
  JSON export of recorded events and frame snapshots.
- `inspector` (feature `inspector`): `InspectorCounters`, a *counting*
  `TreeObserver` — mounts/moves/rebuilds-per-cause/unmounts. This is a
  **counter, not a tree walker or visual inspector**: the README states
  plainly "Nothing here walks a widget, element or render tree, opens a
  port, or watches files."

All three are opt-in `tracing`-layer/library adapters an embedder wires
into their own app manually (code samples in the README show exactly this).
**There is no equivalent of Flutter DevTools' widget inspector UI, layout
explorer, or network/memory tabs** — FLUI's "devtools" is instrumentation
plumbing, not a tool. This matches (and the CLI code itself corroborates)
the premise of issue #1125 that the README overstated the feature; the
current README/FEATURES.md text is now honest about the gap (it reads like
it was already rewritten to fix the overstatement — the CLI command's own
doc comments cross-reference `crates/flui-devtools/FEATURES.md` for "the
real API").

## 6. Testing API for app developers (`crates/flui-testing/`)

`flui-testing` is a real `WidgetTester`-equivalent, not a stub:
- `HeadlessBinding::pump_frame(dt)` — deterministic, sleep-free frame
  advance driven by a virtual `ManualClock` (no flaky real-timer tests for
  long-press/double-tap/animations).
- `bootstrap::mount_root` — canonical way to mount a root `View` into a
  laid-out tree.
- `replay` — scripted gesture replay at explicit virtual-time offsets.
- `a11y` — query the assembled semantics tree (AccessKit nodes) by role.
- Layering rule enforced by `just inventory-check`: production/framework
  crates may only take a **dev-dependency** edge into `flui-testing` (never
  shipped in a release binary); `flui_widgets::testing` is the
  widget-mounting entry point built on top of this, since `flui-testing`
  itself must never depend on the widget catalog.

**Concrete example found**: the generated `flui create` counter app's own
`#[cfg(test)] mod tests` (in the scaffolded `src/lib.rs`, not a separate
examples dir) is the best available worked example. It uses
`flui::testing::widgets::{lay_out, tight}`, then:
```rust
let mut app = lay_out(app_tree(480.0, 320.0), tight(480.0, 320.0));
app.find_text("0")            // find-by-text (byType/byKey not confirmed to exist)
app.dispatch_pointer_down(x, y); app.dispatch_pointer_up(x, y); app.tick();
app.pump_widget(app_tree(...))  // re-pump with a new tree, assert state survives
```
This is functionally equivalent to Flutter's `testWidgets` +
`WidgetTester.pump/tap/find.text`. I did not find a `find.byType`-style
typed finder or a golden-image (screenshot) test facility in
`flui-testing`'s public API from this pass — the README explicitly lists
"golden-image helpers belong here as they land" as **not yet implemented**.
So: pump ✅, tap/gesture replay ✅, enterText not verified, find-by-text ✅,
find.byType not confirmed, golden tests ❌ (explicitly future work per the
README).

## 7. Diagnostics on layout overflow / unbounded constraints

No Flutter-style "A RenderFlex overflowed by N pixels" user-facing
diagnostic exists. What's there instead:
- `RenderFlags::HAS_OVERFLOW` (`crates/flui-rendering/src/storage/flags.rs`)
  — a bit flag, described in its own doc comment as "debug only" — is
  set/cleared but (in the code paths inspected) not converted into any
  logged message, banner overlay, or structured diagnostic. It is silent
  bookkeeping, not a warning.
- `box_protocol.rs`'s `debug_assert_layout_output` — a `debug_assert!` that
  **panics in debug builds** if a render object commits an infinite or
  NaN/otherwise-invalid geometry ("catches the silent-commit of an infinite
  or constraint-violating size" per its own doc comment), and is a
  **silent no-op in release builds** (classic `debug_assert!` semantics —
  release builds get the wrong-but-uncaught geometry instead of a panic).
- Net effect: a beginner who overflows a `Row`/`Column` in FLUI today either
  gets an unhelpful `debug_assert!` panic with no widget-tree context (debug
  build), or silently wrong layout with no warning at all (release build).
  There is no equivalent of Flutter's yellow/black hazard-stripe overflow
  banner or the descriptive stdout diagnostic naming the offending render
  object and by how many pixels it overflowed.

## 8. `docs/getting-started.md` — critical read as a newcomer

Strengths: clear prerequisites table (Rust 1.97 MSRV matches
`rust-toolchain.toml`, wgpu 29.x noted, cargo-ndk/wasm-pack called out as
platform-specific-only), a `RUST_LOG` section, a troubleshooting table.

Gaps/confusions found:
- **No mention of the `lld` linker requirement on macOS** (see §3) even
  though `.cargo/config.toml` hardcodes `-fuse-ld=lld` for both macOS
  targets. This is the single most likely first-command failure for a new
  macOS contributor and isn't in the troubleshooting table.
- The doc's very first substantive instruction, `cargo build --workspace`,
  is followed by "Several crates are intentionally disabled in `Cargo.toml`
  while integration is in progress; see `crates.md`" — a newcomer has no
  way to know *before* the build whether their long build was against a
  "current" or partially-disabled workspace; this caveat should probably be
  a warning box, not a trailing sentence.
- The doc is about **building the framework itself** (running its bundled
  examples), not about **using `flui create`/the CLI** to scaffold a new
  app — the two on-ramps (contributor vs. app-developer) aren't
  cross-linked from here; a newcomer who just wants "make an app" has to
  already know to look at the CLI or README instead.
- `wgpu crashes or shows blank window` troubleshooting entry only suggests
  driver update / `cargo update -p wgpu`; no mention of the (real, observed
  elsewhere in project memory) macOS hidden-window/Metal-drawable and
  frame-pump gotchas already tracked as resolved issues — a newcomer
  hitting a blank window has no signal whether it's a known, already-fixed
  class of bug or new.

## Appendix: raw timings

| Step | Time |
|---|---|
| `cargo build -p flui-cli` (workspace, with lld workaround) | 31.7s wall, 121s user, 399% CPU |
| `flui create hello --no-check` | <1s |
| `cargo check` in generated project (cold, git-tag dep, ~140 crates) | 74.5s wall, 245s user + 32s sys, 373% CPU |
