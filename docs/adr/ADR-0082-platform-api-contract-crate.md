# ADR-0082: `flui-platform-api` is the contract crate; OS backends stay in `flui-platform`

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §2,
  [ADR-0031](ADR-0031-platform-haptics-capability-and-system-chrome-deferral.md) §1–§3,
  [ADR-0038](ADR-0038-data-transfer-architecture.md) §4 and §9, and
  [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §1–§2 (where the traits live, the
  `Send` bound on registered callbacks, and `capabilities`, which §2 keeps on `Platform` and §5
  below deletes); [ADR-0037](ADR-0037-presentation-ownership-domains.md)
  (the `interaction -> platform` edge becomes `interaction -> platform-api`)
- **Related:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md),
  [ADR-0047](ADR-0047-unified-execution-services.md),
  [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md),
  [ADR-0071](ADR-0071-macos-binds-appkit-through-objc2.md),
  [ADR-0072](ADR-0072-presentation-native-execution.md),
  [ADR-0073](ADR-0073-uikit-scene-ownership.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0084](ADR-0084-open-capability-seam-and-plugins.md),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md)
- **Refs:** decision D1 of the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md);
  index in [`design/decisions.md`](../../design/decisions.md)

## Context

`flui-platform` holds two different things in one crate: the contracts the framework programs
against (`PlatformWindow`, `PlatformTextInput`, `PlatformHaptics`, `Clipboard`, the input
vocabulary) and every OS backend that implements them (`src/platforms/` has `windows`, `macos`,
`linux`, `winit`, `android`, `ios`, `web` and `headless`). Anything that names a contract links
all of it.

One import does exactly that below the host tier. `flui-interaction` depends on `flui-platform`
(`crates/flui-interaction/Cargo.toml:31`) for `use flui_platform::traits::PlatformTextInput;`
(`crates/flui-interaction/src/text_input.rs:27`), its only `flui_platform` import. The widget
harness adds a second edge under its `testing` feature (`crates/flui-widgets/Cargo.toml:94`),
for a fake text input (`crates/flui-widgets/src/testing/harness.rs:157,170`). Because
`flui-platform`'s default feature is `desktop = ["dep:winit"]`
(`crates/flui-platform/Cargo.toml:300,303`), winit and the `windows` crate are in the normal
graph of `flui-interaction`, `flui-view` and everything above them: on this host
`cargo tree -p flui-interaction -e normal -i winit --locked` prints
`winit <- flui-platform <- flui-interaction`. Headless and wasm cleanliness of the UI stack is
therefore an accident, a plugin that wants one capability trait would link every backend, and
an edit to the Win32 backend rebuilds the UI stack.

The contracts themselves carry backend and threading assumptions that a contract crate cannot
keep:

- `Platform: Send + Sync + 'static` (`crates/flui-platform/src/traits/platform.rs:253`) and
  `PlatformWindow: Send + Sync` (`crates/flui-platform/src/traits/window.rs:169`) register
  callbacks as `Box<dyn FnMut(..) + Send>`: `set_exit_policy_hook`, `on_quit`, `on_reopen`,
  `on_window_event`, `on_open_urls` (`platform.rs:319,523-550`) and the window's `on_input`,
  `on_request_frame`, `on_resize`, `on_close` and the rest (`window.rs:481-657`). Delivery is on
  the owner thread by construction (ADR-0039 §2), so the bound only forces callers to be `Send`.
  The runner's own comment names the consequence: "the platform callback surface still requires
  `Send`, so the `!Send` realm this holds remains in owner TLS"
  (`crates/flui-app/src/app/runner/host.rs:32-34`).
- `PlatformWindow` names winit under a feature (`fn as_winit(&self) -> Option<&Arc<Window>>`,
  `window.rs:767-770`) and returns `PlatformAccessibility`, which speaks AccessKit
  (`window.rs:352`).
- A second, older window family (`Window`, `WindowManager`, `WindowBuilder`, a crate-local
  `RawWindowHandle` enum, `crates/flui-platform/src/window.rs:53,265,340,452`) exposes
  `fn raw_window_handle(&self) -> RawWindowHandle` (`window.rs:196`). Nothing outside the crate
  uses it; neither `PlatformEmbedder` (`src/traits/embedder.rs:33`), `PlatformCapabilities`
  (`src/traits/capabilities.rs:13`, returned by `Platform::capabilities`, `platform.rs:557`),
  `BasicVelocityTracker` (`src/traits/input.rs:305`) nor `LinuxPlatform`
  (`src/platforms/linux/mod.rs:108`) has a user outside the crate (`grep -rln` over `crates`,
  `src`, `examples`, `tools`). `LinuxPlatform::new` is an `unimplemented!` stub
  (`linux/mod.rs:120-124`); Linux runs on the winit backend, which `flui-app` enables per target
  (`crates/flui-app/Cargo.toml:159`).
- More than one backend can serve an OS: `winit-backend` is "optional on Windows/macOS"
  (`crates/flui-platform/src/platforms/mod.rs:28-32`), so a Windows build can select a backend
  CI never exercises there.
- `flui-platform` starts its own tokio runtime (`crates/flui-platform/src/executor.rs:60-70`),
  which ADR-0047 marked for removal along with `Task` and `BackgroundExecutor`.

## Decision

### 1. A new crate, `flui-platform-api`, holds the contracts

`flui-platform-api` is tier C, kind `stable` (ADR-0081). It contains what the framework, plugins
and tests program against, and nothing that names an OS, winit, AccessKit or tokio:

- the per-window contract `PlatformWindow`, without `as_winit` and without `accessibility()`;
- the capability traits `PlatformTextInput`, `PlatformHaptics`, `PlatformDisplay`, `Clipboard`,
  `DataTransferSource` and their vocabulary (`DataTransferOffer`, `ClipboardItem`);
- the input and window vocabulary: `PlatformInput`, `DispatchEventResult`, `DragDropEvent`,
  `WindowEvent`, `WindowId`, `WindowOptions`, `WindowMode`, `WindowAppearance`, `WindowBounds`,
  `WindowExecutionState` (the single lifecycle state machine of ADR-0035), `CursorError`, and the
  error types those signatures return;
- the capability seam of ADR-0084 (`PlatformCapability`, `CapabilityProvider`, `Unsupported`).

Its public signatures follow ADR-0089: `raw-window-handle` 0.6 only through
`HasWindowHandle`/`HasDisplayHandle`/`HandleError`, `cursor-icon` and `serde` as allowed 1.x
dependencies, and no `ui-events`, `keyboard-types`, `dpi` or `accesskit` type. Where today's
vocabulary re-exports one of those, the own type is introduced before the crate's first
release, not in the move.

### 2. `flui-platform` keeps the backends and the host-facing surface

`flui-platform` becomes tier H, kind `internal`. It keeps every OS backend, the host-facing
`Platform` trait, `OwnerPlatform`, `SharedPlatform`, `PlatformProxy`, `PendingWindow`/`WindowOpen`,
the prompt APIs, and `PlatformAccessibility` (its only consumer is the runner, and its signatures
are AccessKit's). Only composition roots depend on it: `flui-app`, and tests that drive a backend.

`Platform` stays effectively sealed, as ADR-0039 §1 describes: an out-of-crate backend cannot mint
the `OwnerPlatform` its `run()` hands to `on_ready`. This record does not open that minting seam;
it stays with #560. Keeping `Platform` out of the Stable crate is what keeps `Task`,
`BackgroundExecutor` and `prompt_for_paths` (ADR-0039 §2) out of the Stable surface.

### 3. First change: move the traits, re-export them

The first change is mechanical. The listed items move to `flui-platform-api` unchanged apart
from dropping `as_winit` and `accessibility()` from `PlatformWindow`; `accessibility()` becomes
a method of a backend-side extension trait in `flui-platform` that the runner uses.
`flui-platform` re-exports every moved item at its old path, so no caller outside the three
edges changes. `flui-interaction` and the widget harness switch to `flui-platform-api`.

Acceptance: `cargo xtask reach` (ADR-0081 §2) is green for tier K under every facade feature
set, which states that `winit` is absent from `flui-interaction`'s normal closure and
`flui-platform` from `flui-widgets`' with all features. These are reach facts rather than
`cargo tree -i` probes, because `cargo tree -i` errors on an absent package instead of printing
nothing. This change is what turns the K reach fact
green; B0 depends on it.

### 4. Second change: registered callbacks lose `Send`, one backend at a time

Every callback registered through `Platform` or `PlatformWindow` is delivered on the owner
thread, so the contract states that and drops the bound: `Box<dyn FnMut(..)>` instead of
`Box<dyn FnMut(..) + Send>`. `PlatformExecutor::spawn` (`platform.rs:698`) keeps `Send`: it
really crosses threads. The traits themselves stay `Send + Sync`; handles such as
`Arc<dyn PlatformWindow>` still reach the raster thread (ADR-0063).

A trait signature cannot differ per backend, so the order is:

1. each backend moves its callback storage to owner-thread-only state, behind an adapter that
   still accepts `Send` callbacks — Win32, AppKit, winit, UIKit, Android and web in separate
   changes, each with the live-run evidence below;
2. one change drops `+ Send` from the signatures once every backend stores callbacks owner-only.

A backend whose storage cannot be made owner-local keeps a `Send` adapter and blocks step 2; that
is the rollback, per backend. When step 2 lands, the runtime's owner host can hold the `!Send`
realm without reaching it from a `Send` closure, and the thread-local cell narrows to the single
trampoline cell of ADR-0083 and ADR-0097.

### 5. One backend per OS

| Target | Backend |
|---|---|
| Windows | `platforms::windows` |
| macOS | `platforms::macos` (ADR-0071) |
| Linux | `platforms::winit`, until a native Wayland/X11 backend has a second reason to exist |
| Android | `platforms::android` |
| iOS | `platforms::ios` (ADR-0073) |
| wasm32 | `platforms::web` |
| tests | `platforms::headless` |

`winit-backend` stops being selectable on Windows and macOS. Deleted, since nothing outside the
crate uses them: `LinuxPlatform`, the `src/window.rs` family (including `RawWindowHandle` and
`fn raw_window_handle`), `PlatformEmbedder`, `PlatformCapabilities` with
`Platform::capabilities`, `BasicVelocityTracker`, and the `desktop` and `web` features, which
have zero `cfg` sites in `crates/flui-platform/src` (ADR-0081 §4). The crate's own tokio runtime
goes with ADR-0047's consolidation, not here.

## Alternatives considered

- **Move the backends into `flui-app`.** The composition root would grow by the whole backend
  code base, with `unsafe` OS glue interleaved with runtime code, and plugins would still have no
  small crate to depend on.
- **One crate per backend** (`flui-platform-windows`, …). The backends share `shared/` modules
  and the `OwnerPlatform` minting rule; splitting them now forces that seam open before #560
  designs it, and multiplies release units for no second consumer.
- **Move `Platform` and `OwnerPlatform` into the contract crate too.** It would freeze about
  thirty methods, `Task` and the prompt APIs into a Stable crate, and still not let an external
  backend implement it, because minting stays sealed.
- **Only drop the default `desktop` feature.** It removes winit from the UI graph but leaves the
  `windows` target dependency, the `objc2` stack and every backend's code linked through the one
  import.
- **Make `Platform` itself `!Send`.** ADR-0039 already rejected it: background resolution of the
  clipboard and shell dispatches is legitimately cross-thread.

## Consequences

- `flui-interaction` and `flui-widgets` stop linking OS crates; `cargo xtask reach` can gate it.
  How many crates an OS-backend edit rebuilds after the split has not been measured; it should be
  measured with `cargo build --timings` on the first change, not asserted.
- `flui-platform` sits in tier H with `flui-app`. Its layer-2 position in ADR-0041's table, and
  ADR-0041's `interaction -> platform` edge note, are obsolete.
- ADR-0030 §2, ADR-0031 §1–§3 and ADR-0038 §4 describe traits that now live in
  `flui-platform-api`; their contracts are unchanged. ADR-0038 §9's required
  `Platform::clipboard()` stays on `Platform` in `flui-platform`. ADR-0039 §2's "registration
  writes a `Send` callback" is replaced by §4 above once step 2 lands.
- Win32, AppKit, UIKit and Android are clippy-only in CI (AGENTS.md). Each backend change in §4
  is unverified unless its PR shows a live run (`cargo xtask device windows-input` on Windows, the
  macOS smoke path on AppKit).
- Downstream code that imported the deleted items has none in this workspace; the re-exports at
  old paths are removed in the minor after the first change, with a CHANGELOG note.

## Verification

None of these exist yet.

- `cargo xtask reach` (ADR-0081) with `flui-platform`, `winit`, `windows`, `objc2-app-kit`,
  `objc2-ui-kit`, `android-activity` and `ndk` forbidden for tier K; red before the first change,
  green after.
- A reach fact for `flui-platform-api` itself: its tier C set forbids the OS crates and winit,
  and its own `reach-forbid` entry (ADR-0081 §2) adds `tokio` and `accesskit`.
- `cargo xtask cross-typecheck` for Win32, AppKit, Android and iOS on every backend change.
- A compile-time pin that registered callbacks accept `!Send` closures once step 2 lands: a
  doctest on `PlatformWindow::on_input` that registers a closure capturing an `Rc`.
- The API-closure gate of ADR-0089 run over `flui-platform-api`, with a planted
  `pub fn f() -> winit::window::WindowId` that it must reject.
