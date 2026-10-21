# ADR-0151: Platform layer: crates, names, the closed list, contents and the core host's layout

- **Status:** Accepted (2026-10-20); not implemented: the renames, the moves, the gates and the
  layout land as separate changes, the renames first (section 12)
- **Date:** 2026-10-20
- **Supersedes:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) §1 (what the contract crate
  holds), §2 (the crates' names and what the core host keeps), §4 step 2 (callbacks without
  `Send`, jointly with [ADR-0254](ADR-0254-window-contract.md)) and §5's Linux row (winit as the
  Linux backend; section 11 here); and the placement of
  `LockArbiter`, `CompositionLedger` and `EditGeneration` in the contract crate by
  [ADR-0142](ADR-0142-text-store-commit-gate-and-owner-code-containment.md) (its other decisions
  stand). The boundary ADR-0082 drew between a contract crate and a backend crate stands.
- **System preferences:** [ADR-0172](ADR-0172-host-owned-system-preferences.md) owns them. The
  Proposed text of this ADR (2026-10-06) had a section on the subject; ADR-0172 replaced it and
  this text does not repeat it.
- **Related:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) (section 13:
  `AppLifecycleState` stays where it is),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (upstream types in Stable
  signatures; section 5 builds its closure gate),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (every `static` has an entry; section 10
  re-keys them), [ADR-0152](ADR-0152-capability-seam-revised.md) (capability seam),
  [ADR-0153](ADR-0153-release-train-and-capability-crates.md) (release train; section 5),
  [ADR-0154](ADR-0154-capability-crates.md) (capability crates; section 3),
  [ADR-0213](ADR-0213-input-capabilities.md) (`Availability`; section 4),
  [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) (display snapshot),
  [ADR-0237](ADR-0237-window-state-and-host-notices.md) (window state; ADR-0254 completes it),
  [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) (test doubles; section 7),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (OS floors; section 14),
  [ADR-0247](ADR-0247-android-java-view.md) (Android Java view; its crate path moves in section 12),
  [ADR-0254](ADR-0254-window-contract.md) (window contract),
  [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (Android, iOS and Web hosts)
- **Sources:** other projects are read at pinned commits and cited as `owner/repo@sha path:line`
  where they bear on a decision: winit 0.31.0-beta.3 (`rust-windowing/winit@1893565716`), GPUI
  (`zed-industries/zed@fae8a35482`), Slint (`slint-ui/slint@277e7676ce`), SDL3
  (`libsdl-org/SDL@d0183f0ee3`), Flutter (`flutter/flutter@6597889f57`), Masonry
  (`linebender/xilem@271a27a6d4`), raw-window-handle 0.6.2
  (`rust-windowing/raw-window-handle@1b859486d7`), Tauri v2 (`tauri-apps/tauri@9318ab84b8`),
  Flutter's packages (`flutter/packages@1383427519`) and Compose Multiplatform
  (`JetBrains/compose-multiplatform-core@13b7599795`). Counts and lines of FLUI's own tree are at
  `main` `083b0f75d`.

## Context and problem statement

ADR-0082 split the contract crate from the backend crate. The boundary was right and stands; what
grew on each side of it since is the subject here. Numbers are counts over tracked files at `main`
`083b0f75d` (`wc -l`, `rg`; the platform crates are unchanged since `deb9faea2`) unless another
source is named.

- **The contract crate** (`flui-platform-api`) has 16,481 lines in `src/`, re-exported whole as
  `flui::platform`. 63% of it is text-store machinery (3,708 lines), keyboard (2,613), pointer
  (2,016), data transfer and drag (1,173) and haptics (868). Items with no production consumer
  above the backend sit in it: `WindowEvent`, `WindowBackgroundAppearance`, the `OfferTable` family, four
  coordinate helpers, and three test doubles (`InMemoryClipboard`, `InMemoryTextStore`,
  `FakeHaptics`). 26 of its 82 public enums are closed with no recorded decision (82 `pub enum`,
  56 of them with `#[non_exhaustive]`, over `crates/flui-platform-api/src`), and nine enums spell
  "unsupported" nine ways: `crates/flui-platform-api/src/window_state.rs:455`,
  `window.rs:283` and `:342`, `drag.rs:48`, `haptics/request.rs:125`,
  `input_capabilities/mod.rs:24`, `storage.rs:300`, `preferences/geometry.rs:361` and
  `text_store/host.rs:81`.
- **The earlier premises have gone stale.** Haptics are implemented on five backends
  (a 1,242-line degradation table, a 909-line consumer in `flui-interaction`). The text-store lock
  machinery has production consumers in `flui-widgets`, `flui-interaction` and the Win32 text
  services. `PlatformDisplay`, `WindowBounds`, the legacy `window.rs` and `PlatformCapabilities`
  are already gone.
- **The window trait** serves the same fact through a getter, an `on_*` registration and
  `state()`; 21 of its 39 defaulted methods do nothing. It is `Send + Sync` for one consumer, the
  raster thread (`crates/flui-platform-api/src/platform_window.rs:205`), and `flui-app` keeps its
  runtime in a `thread_local!` because of the `Send` callbacks
  (`crates/flui-app/src/app/runner/host.rs:431-441`). The window side is
  [ADR-0254](ADR-0254-window-contract.md)'s.
- **The backend crate** (`flui-platform`) has 97,363 lines in `src/` (`platforms/` 72,528,
  `shared/` 20,320, `traits/` 1,709), the largest crate in the workspace. 45% of `shared/` (about
  9,100 lines) is used by exactly one OS. 21 rule modules are `pub #[doc(hidden)]` only so
  `tests/` can name them, which compiles about 3,900 lines of single-OS rules on every target.
  `LinuxPlatform` is a 274-line `unimplemented!()` stub
  (`crates/flui-platform/src/platforms/linux/mod.rs`) inside a 7,433-line directory of live code
  (displays, logind notices, the AT-SPI adapter). Six separate lease helpers hand user callbacks
  out of their slots (`RunningSessionEndHook`, `CallbackLease` and `InputLease` in
  `shared/handlers.rs:236,646,712`; `WindowEventLease` in `platforms/headless/platform.rs:1850`;
  `WindowEventHandlerLease` and `ExitPolicyHookLease` in `platforms/winit/platform.rs:747,776`), and
  user code still runs under a lock in two places
  (`platforms/android/mod.rs:1010`, `platforms/headless/platform.rs:748-750`).
- **`Platform` outgrew its callers.** No production code above the backend calls
  `background_executor`, `active_window`, `write_to_clipboard`, `read_from_clipboard`, `open_url`,
  `reveal_path`, `open_path`, `prompt_for_paths`, `prompt_for_new_path`, `on_window_event`,
  `keyboard_layout`, `on_keyboard_layout_change`, `app_path` or `compositor_name`; `on_open_urls`
  has no producer (`invoke_open_urls` is never called). Application-level events and queries
  arrive through nine separate registrations (`crates/flui-platform/src/traits/platform.rs`:
  `on_quit`, `on_session_end`, `on_host_notice`, `on_reopen`, `on_window_event`, `on_open_urls`,
  `on_keyboard_layout_change`, `set_exit_policy_hook` and `set_wake_deadline_hook`), plus the
  one-shot `PlatformReadyCallback` that `run` takes.
- **There is no capability seam in the code.** ADR-0152, ADR-0153 and ADR-0154 are accepted with
  this record and not implemented.
- **The names.** `flui-platform-api` is a three-part name; the rule is `flui-<word>`. None of
  `flui`, `flui-native`, `flui-platform` or `flui-platform-api` is on crates.io (404, checked
  2026-10-20).

The question this record answers: which crates make up the platform layer, what does each hold and
what is it called, and how is the core host laid out, so that each side of the boundary ADR-0082
drew holds only what belongs on it?

## Decision drivers

- Only `flui-app` depends on the core host and the seam is sealed, so there is no out-of-tree
  backend and no second consumer of a backend's internals (section 1).
- A contract item needs a production consumer above the backend; an item without one is a promise
  nobody tests (section 2).
- The contract is Stable: the size of the promise is the size of its API closure, and no public
  signature names an upstream type beyond ADR-0089 §2's exceptions (sections 2 and 5).
- A service is in the core host only when delivering it needs an object that only the host owns,
  or when the framework's own widgets consume it without the application wiring it (section 3).
- Crate names follow `flui-<word>`, and `flui-native` and `flui-platform` are free on crates.io.
- The contract depends on `flui-foundation`, which carries `links = "flui_train"`, so its version
  rides the release train until `cargo-semver-checks` shows it stable (section 5).
- CI executes Linux and wasm32 only: the rules of every OS must compile and run on the Linux CI
  host, and the native shells need local runs (sections 10 and 16).
- Rules are types: a seal, `clippy::exhaustive_enums` and the `module-dag` gate keep the halves of
  the layer apart before a reviewer has to (sections 5 and 10).
- User code never runs with a lock or a borrow held, and a panic in it never unwinds into the OS
  (sections 10 and 16).
- Absence has one spelling, and the contract never carries an OS version (section 4, ADR-0244).
- Web and Wayland clipboards answer only asynchronously, so a synchronous `Clipboard` cannot be
  honest there (section 7).
- No process-global state: every `static` has a `globals` entry, and a move re-keys it (section 10,
  ADR-0097).
- Accessibility must not depend on an author knowing that a feature exists (section 15).
- Support is claimed only by an executed run, and the qualities of section 16 are requirements, not
  a later pass.
- Open branches must be able to repeat a rename mechanically, and `git blame` must survive it
  (section 12).

## Considered options

1. The restructuring this record decides: two framework platform crates (`flui-platform` for the
   contract, `flui-native` for the core host), a closed list of OS services, one crate for every
   native backend and a layout with `rules` and `shell` halves.
2. Keep `flui-platform-api` and `flui-platform`.
3. Name the contract `flui-port`, `flui-contract` or `flui-core`.
4. Name the core host `flui-os` or `flui-backend`.
5. A crate per OS (winit, GPUI).
6. Per-OS crates behind a facade.
7. A pure `rules` crate plus FFI shells.
8. Keep `shared/` or rename it `backend/`.
9. Everything in the core host (GPUI's 76-method `Platform`).
10. Menus and haptics as capability crates.
11. A contract that anticipates consumers.
12. Its own contract version at once.
13. A hidden second tier for first-party backends (Slint).
14. A synchronous `Clipboard` with an asynchronous twin (GPUI).
15. Keep the lock, ledger and generation in the contract, or move them to `flui-widgets`.
16. `AppLifecycleState` in the contract.
17. Keep winit as the Linux backend.
18. One pull request for both renames.
19. A `cargo xtask rename` tool.
20. `cfg` by OS version, and "latest installed SDK" for `targetSdk`.
21. `a11y` stays a feature, off by default.
22. Quality after the features.

## Decision

Chosen option: 1, because it keeps the boundary ADR-0082 drew and puts every item on the side where
its consumer is: the contract holds only what something above the backend uses, the core host does
what needs an object only it owns, everything permission-gated or hardware-bound is a capability
crate, and the compiler and the gates named under Confirmation keep the sides apart.

This ADR decides which crates exist and what they are called, what the core host does with the OS,
what the contract holds and what happens to each of its modules, how absence is spelled, how the
contract stays stable, how the core host is laid out, that accessibility is always built, and which
qualities every platform must have. Decided elsewhere: the window contract
(state, commands, one handler, the `!Send` window and its surface target) in
[ADR-0254](ADR-0254-window-contract.md); the capability seam, the capability crates and the host
bridge in [ADR-0152](ADR-0152-capability-seam-revised.md) and
[ADR-0154](ADR-0154-capability-crates.md); the Android, iOS and Web hosts in
[ADR-0255](ADR-0255-mobile-and-web-hosts.md); system preferences in ADR-0172.

### 1. Two framework platform crates; one crate for every native backend

| Role | Before | After | Tier / kind | Layer |
|---|---|---|---|---|
| Contract | `flui-platform-api` | **`flui-platform`** | C, `stable` | 1 |
| Core host (every native backend) | `flui-platform` | **`flui-native`** | H, `internal` | 3 |
| Capability crates | none | `flui-system`, then others (ADR-0154) | `pkg`, `capability` | n/a |

The contract takes the name of the path users already write (`flui::platform::WindowOptions`).
The core host is the code that talks to the OS. There is no `-core` crate and no crate per OS.

Carried over from ADR-0082 §2, which this section replaces: only `flui-app` depends on the core
host, apart from test and example dev-dependencies that drive a backend (`allowed-dependents`);
`PlatformAccessibility` and its listener aliases live in `flui_semantics::platform`, while
`HostWindow::accessibility` stays in the core host; an out-of-crate backend cannot mint the
`OwnerPlatform` its `run` needs, and section 5 makes that seal explicit.

**One crate for Windows, macOS, iOS, Android, Web, Linux and headless.** winit 0.31 and GPUI
split into a crate per backend on top of an open backend seam, for out-of-tree backends and for
users of its types without the event loop (winit:
[rust-windowing/winit#3433](https://github.com/rust-windowing/winit/issues/3433), the split, and
[#3367](https://github.com/rust-windowing/winit/issues/3367), the API redesign it follows) and for
dependents that need not wait for platform crates (GPUI:
[zed-industries/zed#2874](https://github.com/zed-industries/zed/pull/2874), landed as
[#49277](https://github.com/zed-industries/zed/pull/49277)). FLUI has neither:

- The seam is sealed (`OwnerPlatform` is minted by a `pub(crate)` constructor) and the only
  dependent is `flui-app`, which needs every backend to link. A split cannot shorten that build;
  it can only overlap work (reasoned, not measured).
- A split makes public what the OS crates reach in the core: 468 lines in `platforms/` name
  `crate::shared::` or `crate::traits::`, the `OwnerPlatform` mint among them.
- The `cfg` does not disappear, it moves to the facade: 70 lines starting `#[cfg` in winit's
  facade (`rust-windowing/winit@1893565716 winit/src`), 19 lines naming `cfg` in `gpui_platform`
  (`zed-industries/zed@fae8a35482 crates/gpui_platform/src/gpui_platform.rs`); and to feature
  forwarding.
- Each crate adds a tier, a layer, `globals`, reach and name-keyed tooling entries (145 lines in 30
  files of `tools/`, `.github/`, `Cargo.toml`, `deny.toml` and `.config/` name `flui-platform`
  today) and a unit in the first publication; ADR-0041 admits a new crate for a new layer or a
  proven second consumer, and these have neither.
- winit's own split came after more than a year of turning its API into traits: the trait
  refactors listed in #3433 preceded the first crate move, `winit-core`
  ([rust-windowing/winit#4208](https://github.com/rust-windowing/winit/pull/4208), merged
  2025-05-14), and the crates share one version
  ([#4274](https://github.com/rust-windowing/winit/pull/4274)). FLUI's seam is closed on purpose.

What FLUI does instead is in section 10: per-OS `rules` and `shell` halves in one crate, with the
compiler and `module-dag` keeping them apart.

The conditions under which this question reopens are listed in More information.

### 2. What the contract holds

Vocabulary and traits that something above the backend uses (applications through the facade
included), and the types by which a backend hands the framework a fact. An item with no such
consumer is `pub(crate)` in the core host or does not exist. "It returns with its first consumer"
is not a state an item may be in: either its consumer is built together with it, or the item is
not there.

The contract holds the vocabulary of windows, input, text, displays, drag and drop, system
preferences, host notices, haptics and system chrome; the window and face traits; and the
capability seam (`Capability`, the provider, `Plugin`, the registrar, `Completion<T>`,
`Unsupported`). It holds no OS code, no `unsafe`, no test doubles, no backend tables and no
synchronous `Clipboard`, and depends on neither `tracing` nor `parking_lot` (section 7 says how
they leave; both go into its `reach-forbid`). No public signature names an upstream type beyond
ADR-0089 §2's exceptions (`raw-window-handle` 0.6 through `HasWindowHandle`, `HasDisplayHandle`
and `HandleError`; `cursor-icon`; `serde` behind a feature), whatever ADR-0089's own status is.

| Module | Fate |
|---|---|
| `brightness`, `locale`, `target_platform`, `event_time`, `keyboard/`, `pointer/`, `ime`, `text_input`, `input_capabilities/`, `display`, `placement`, `window_state`, `host_notice`, `preferences/`, `system_chrome`, `haptic_feedback`, `haptics/`, `drag`, `storage` | Stay. `keyboard` loses `KeyText`; `display` moves to `std::sync::Mutex`; `haptics/` loses `FakeHaptics` |
| `window`, `platform_window` | Reworked by ADR-0254: snapshot, `AdmittedCommand`, `WindowHandler`, `!Send` window and `SurfaceTarget`. `WindowEvent`, `WindowBackgroundAppearance`, the getters and the command twins are removed |
| `input` | `PlatformInput`, `DispatchEventResult` and `DragDropEvent` stay; the coordinate helpers move to the core host |
| `input_regions` | `InputRegion` and `InputRegionKind` stay; `DeviceInputRegion` and `ProjectedInputRegions` move to the core host |
| `data_transfer` | The vocabulary stays; `OfferTable`, `OfferRecord`, `TransferCompleter`, `TransferUri` and `TransferImage` move to the core host |
| `clipboard` | Removed. Clipboard is a service behind the seam: its handle in `flui-interaction`, its provider in `flui-app` over the core host's clipboard (section 7) |
| `text_store/` | Section 6 |
| `unsupported` (new) | `Unsupported`, `UnsupportedReason`, `Availability` (section 4) |
| `capability` (new) | `Capability`, `CapabilityProvider`, `ProviderContext`, `WindowRef`, `Plugin`, `CapabilityRegistrar`, `Completion<T>` and, with the first capability that needs a permission, `PermissionState` and relatives (ADR-0152) |
| `menu` (new) | The menu model (menu bar, context menus, Dock menu), landed together with its first implementation and its consumer |

`storage` stays in the contract as listed; section 7 says how it reaches widgets.

### 3. What the core host does with the OS: a closed list

**The membership rule.** A service is in the core host when delivering it needs an object that only
the host owns (the application delegate, the Activity, the window procedure, the event loop), or
when the framework's own widgets consume it without the application wiring it. Anything else is a
capability crate (ADR-0154). By that rule the core host's OS integration is exactly this list:

- **Launch and activation:** delivery of URLs and files on cold and warm start, reopen, handing
  arguments to the single running instance.
- **Session and host:** session end, host notices (memory pressure, sleep, wake).
- **Windows:** state, commands, decorations and a custom title bar, background material, child
  and popup windows.
- **Displays and frame rate.**
- **Input:** pointer, keyboard and layout, IME and the text store, drag and drop.
- **Accessibility, clipboard, haptics, system chrome, system preferences.**
- **Application menus:** the menu model, the menu bar, context menus and the Dock menu, which the
  application delegate hands out.
- **Surfaces and data directories.**

What the rule leaves out follows from it. The Windows jump list is not on the list:
`ICustomDestinationList` needs no object the host owns, so it is a capability and lives in
`flui-system`. The tray is a capability too, and it enters `flui-system` together with the menu
feature: without the menu model a tray is only an icon. Opening a URL, a file or a folder in the
system's handler is a capability; a `Link` widget names its handle through ADR-0152.

The same line falls in the references. Tauri's core holds windows, menus, the tray behind a feature
and the `RunEvent::{Opened, Reopen, ExitRequested}` activation events, while dialogs, clipboard,
notifications, opener and geolocation are plugins (`tauri-apps/tauri@9318ab84b8
crates/tauri/src/app.rs:220-295`, `crates/tauri/Cargo.toml:203`). Flutter's framework holds
clipboard, haptic feedback, system chrome and the platform menu bar, and the rest is packages
(`flutter/flutter@6597889f57 packages/flutter/lib/src/services` and
`packages/flutter/lib/src/widgets`; `flutter/packages@1383427519 packages`). Compose keeps
clipboard, haptic feedback and the URI handler as locals in its `ui` module
(`JetBrains/compose-multiplatform-core@13b7599795
compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/platform/CompositionLocals.kt:74-269`).
GPUI puts everything in one 76-method `Platform` (`zed-industries/zed@fae8a35482
crates/gpui/src/platform.rs:363-604`), which is the shape this list refuses. The menu bar is core
because a macOS application without one has no Quit or Edit menu (the App menu bar row of
`docs/platform-parity.md`).

What is permission-gated, hardware-bound or legitimately absent on a supported platform is a
capability crate. The first one, `flui-system`, takes `open_url`, `reveal_path`, `open_path`, the
file prompts and `app_path` off `Platform`, plus the message box, trash, share, launch at login,
global shortcuts, taskbar progress and badge, the jump list, and the tray when the menu feature
lands. A capability whose handle framework widgets must name keeps its marker and handle in the
lowest crate that names it (ADR-0152); only the OS provider lives in the capability crate.

### 4. One vocabulary for absence

`Unsupported { reason }` is the one type that says "not here", embedded in every error that can
mean absence, in place of the nine spellings found today (`WindowCommandError`,
`WindowShowError`, `CursorError`, `DragError`, the haptic outcome enum, `StorageError`,
`PreferenceQueryError`, `TextStoreHostError` and `Availability`). Its reasons are
`NotOnThisPlatform`, `NotOnThisDevice` (the OS build or the hardware of this device lacks it; the
value names no version), `NotDeclared { what }`, `Unavailable` (temporary; trying again may
succeed), `NotRegistered` and `NoWindow`. It is built with `Unsupported::new(reason)`, or with
`Unsupported::of::<C>(reason)` when a capability's name belongs in the message (ADR-0152).
`Availability` (native, emulated, unavailable) moves from the input capabilities into the
`unsupported` module and also describes `WindowCapabilities`, so a window can answer "emulated"
where a boolean cannot (ADR-0254).

The contract never carries an OS version. Version gates stay proof types inside the core host
(ADR-0244), so no application can write `if sdk >= 29`. For the same reason there is no reason
named after a version: a gate that refused answers `NotOnThisDevice`, which says that the device
lacks the thing and invites no question about which version has it.

winit embeds one `NotSupportedError { reason }` in three error enums
(`rust-windowing/winit@1893565716 winit-core/src/error.rs:178-236`), and raw-window-handle
separates "cannot be represented" (`HandleError::NotSupported`) from "temporarily unavailable"
(`HandleError::Unavailable`; `rust-windowing/raw-window-handle@1b859486d7 src/lib.rs:320-345`).
FLUI already has the retryable class in three places under three spellings
(`HandleError::Unavailable`, `StorageError::Busy` at `crates/flui-platform-api/src/storage.rs:310`,
the bounded-retry rule of the gesture geometry at `platform_window.rs:238-241`); `Unavailable` is
that class named once.

### 5. Stability: closed enums decided, the closure gated, the seal explicit, the version with the train

- **`clippy::exhaustive_enums`** is enabled in the contract, with winit's rule
  (`rust-windowing/winit@1893565716 winit-core/src/lib.rs:9-14`). A closed enum carries an
  `#[allow]` with the reason its variants can never grow; every other public enum is
  `#[non_exhaustive]`. 26 of 82 are closed today with no such reason.
- **The API-closure gate** of ADR-0089 §6 is built before the contract's first publication. The
  size of the promise is the size of the closure; it is not built today.
- **Sealing.** `Platform` and `HostWindow` are sealed explicitly, by a private supertrait. They
  are already unimplementable outside the crate (`OwnerPlatform::new` is `pub(crate)`) and only
  backend modules implement them, so the seal costs nothing and makes the compile error say what
  the types enforce. A sealed trait can be opened later without breaking anyone; an open one can
  be neither sealed nor given a required method without breaking every implementor.
  `PlatformWindow` stays open: 15 test
  doubles in 13 files outside the backends implement it, and its doctest promises that. For an
  open trait: no inert defaults, and any facet beyond the required core is a separate trait
  reached through an `Option`, so adding a facet never breaks an implementor. There is no hidden
  richer vocabulary for first-party backends: Slint's `WindowAdapterInternal` and its hidden
  `InternalEvent`, which carries the touch finger id, drag payloads and input method composition,
  are the pattern to avoid (`slint-ui/slint@277e7676ce internal/core/window.rs:225-341`,
  `internal/core/platform.rs:564-575`), and `PlatformInput` carries pointer, key, IME and drag to
  every backend alike. Slint seals its `Renderer` the way
  `Platform` is sealed here (`internal/core/renderer.rs:43-52`).
- **Versioning.** The contract's version rides the release train until `cargo-semver-checks`
  shows it stable for two trains in a row; then it gets its own version (ADR-0153 option 1,
  entered when that evidence exists; ADR-0153 keeps the cases and the measurement). Only
  dependency-free raw-window-handle (`rust-windowing/raw-window-handle@1b859486d7 src/lib.rs:28-33`)
  and the two ABI-disciplined C APIs version apart from their implementations: Flutter's embedder
  header writes its ABI rules down (`flutter/flutter@6597889f57
  engine/src/flutter/shell/platform/embedder/embedder.h:12-48`), and SDL tags every symbol `\since`
  (`libsdl-org/SDL@d0183f0ee3 include/SDL3/SDL_version.h:139-160`). winit-core and its backends
  share one version (`rust-windowing/winit@1893565716 winit-core/Cargo.toml:13`). The contract
  depends on `flui-foundation`, which carries `links = "flui_train"`, so an independent version at
  once is not available.

### 6. Text store: backend machinery stays, widget machinery moves down to `flui-interaction`

Stay in the contract, because backends call them: the store, host and observer traits and their
values, `CommitGate`, `OwnerCalls`, `commit_composition_in_place` and `project_ime_event` (the
Win32 text services, macOS, Android and Web use them).

Move to `flui-interaction::text_input`, because only `flui-widgets` calls them: `LockArbiter`,
`CompositionLedger`, `EditGeneration`, `byte_range` and `committed_text`, about 1,600 lines of
policy with one consumer. `flui-sdk` re-exports them for authors of text widgets. This replaces
ADR-0142's placement of the three types in the contract; ADR-0142's decisions (the commit gate,
containment, the owner hearing committed text) stand. `InMemoryTextStore` moves to `flui-testing`
(section 7).

### 7. Services, test doubles, backend-only items and deletions

- **Services reach widgets through the seam.** Clipboard and storage are application services:
  widgets reach them only through the capability seam. The providers are assembled by `flui-app`,
  the composition root, over OS objects the core host exposes: an owner-local clipboard that reads
  through a callback, and `FileStore`. They cannot be built-in providers of the core host, because
  the marker `ClipboardCapability` sits in `flui-interaction`, which the core host cannot depend on
  (ADR-0152). The synchronous `Clipboard` trait is deleted: `read_text` cannot be implemented
  honestly on the Web and Wayland, which is why GPUI added an async twin
  (`zed-industries/zed@fae8a35482 crates/gpui/src/platform.rs:573-583`) and Masonry made paste an
  event (`linebender/xilem@271a27a6d4 masonry_core/src/app/render_root.rs:240`,
  `masonry_core/src/core/events.rs:40`). Facets of a window (text input, haptics, system chrome,
  drag source) stay facets of the window.
- **Test doubles leave the contract.** `InMemoryTextStore` moves whole to `flui-testing`
  (ADR-0243). `FakeHaptics` and `InMemoryClipboard` each split in two roles: a headless
  implementation, `pub(crate)` in the core host (the headless host is a runtime-selected backend
  and cannot depend on `flui-testing`), and a scripted double in `flui-testing`. The roles differ:
  the buffer of a headless host against the sequence of answers a test scripts.
- **Backend-only items move into the core host as `pub(crate)`:** `OfferTable`, `OfferRecord`,
  `TransferCompleter`, `TransferUri`, `TransferImage`, `device_to_logical`, `logical_to_device`,
  `offset_from_coords`, `delta_offset_from_coords`, `DeviceInputRegion`, `ProjectedInputRegions`,
  and the drag provider hooks `OsDragProvider` and `OsDragEndHook`, which nothing above the backend
  names at `083b0f75d` (`PlatformDragSource` and the drag vocabulary stay).
- **Deleted:** `WindowEvent`, `Platform::on_window_event`, `KeyText`, and
  `WindowBackgroundAppearance`, which a background-material command gated by a window capability
  replaces (ADR-0254).
- **Kept, and wired.** `WindowOptions::input_latency` has no setter above the backend today (only
  the Android and Web backends read it). It stays in the contract, and the application's window
  configuration, its consumer, sets it in the same series.
- **Dependencies.** The contract does not log: the caller of a contract function does, so
  `tracing` goes. `DisplayRegistry` moves from `parking_lot` to `std::sync::Mutex`, the last use
  once the doubles have left. Both names join the contract's `reach-forbid`.

### 8. The escape hatch is published, typed and cannot panic

- `as_any` moves from `PlatformWindow` to the sealed `HostWindow`.
- The facade publishes the OS extension modules as `flui::native::<os>` behind `cfg(<os>)` and the
  feature `native`, kind Evolving, so the escape hatch is discoverable and its churn is not Stable.
  Their methods take and return FLUI types only (progress on the taskbar, background material,
  macOS tabs) and return `Option` or `Unsupported`; none panics on a foreign backend (winit
  `unwrap`s its downcast: `rust-windowing/winit@1893565716 winit-win32/src/lib.rs:239,355-358`).
- Raw window and display handles leave only through the `raw-window-handle` traits, the one
  upstream exception of ADR-0089 §2.
- Creation options for one OS are a typed value of that OS in `WindowOptions`, not per-OS fields
  in the common struct (`zed-industries/zed@fae8a35482 crates/gpui/src/platform.rs:2557-2640`) and
  not string keys (SDL: `libsdl-org/SDL@d0183f0ee3 include/SDL3/SDL_video.h:1493-1507`).
- OS types in the core host's public paths (`HWND` and the `win32` re-exports at
  `platforms/windows/mod.rs:50-59`, `NSApplication`, `AndroidApp`, `HtmlCanvasElement`,
  `tokio::runtime::Handle`) become `pub(crate)`. Deeper control from outside goes through
  capability crates and the host bridge (ADR-0152).

### 9. `Platform` slims; one `AppHandler` replaces the application-level registrations

Removed from `Platform`: `background_executor` with `BackgroundExecutor` and `Task`,
`active_window`, `write_to_clipboard`, `read_from_clipboard`, `on_window_event`,
`compositor_name`, and `on_open_urls`, which has no producer. Moved to `flui-system`: `open_url`,
`reveal_path`, `open_path`, `prompt_for_paths`, `prompt_for_new_path`, `app_path`.
`keyboard_layout` and `on_keyboard_layout_change` become `pub(crate)` in the core host: nothing
above the backend reads them today. The menu feature, which labels shortcuts, publishes them with
that consumer.

The application-level registrations that remain (`on_quit`, `on_reopen`, `on_session_end`,
`on_host_notice`, `set_exit_policy_hook`, `set_wake_deadline_hook`) and the one-shot
`PlatformReadyCallback` are replaced by one `AppHandler` passed to `run`, for the reason
`WindowHandler` replaces the window's fourteen: a required method cannot silently drop what the
framework handed it, there is one slot and one drop order, and nothing is registered late.

`AppHandler` is a trait of the core host (`flui-native`), not of the contract: it names
`OwnerPlatform`, and only `flui-app` implements it. Its obligations are exactly what `Platform`
registers today, as required methods with no default body: `ready(OwnerPlatform)` (the bootstrap
that `PlatformReadyCallback` carried), `quit_requested`, `reopen`, `session_end` (answering with
`SessionEndAnswer`), `host_notice`, and the two queries the loop asks, `exit_policy` and
`wake_deadline`. Nothing else is on it. `opened(urls, files)`, the delivery of activated URLs
and files, joins the handler with the activation feature, together with its producer and its
consumer, the router; it is not added now as an empty method, which would be the inert default
section 7 and ADR-0254 remove. System preferences keep the delivery ADR-0172 decided; no
`on_preferences_changed` exists and none is added.

The core host stops re-exporting contract names: every item has one path.

### 10. The layout of `flui-native`

```text
crates/flui-native/
  android/                  Java sources, committed classes.dex and its hash record (outside src/)
  src/
    lib.rs                  current_platform(): the one backend selection ladder
    platform.rs             Platform (sealed), AppHandler
    host_window.rs          HostWindow (sealed), as_any
    owner/                  OwnerPlatform, SharedPlatform, PlatformProxy, the owner signal and slot,
                            the panic boundary, the one call point for user code
    mapping/                what two or more OS families use: pointer_assembly, keys,
                            haptic_degradation, display_source, window_state, input_availability,
                            system_notice, os_version
    storage/                FileStore, the Web page store
    platforms/
      mod.rs                one gate per backend
      windows/              rules/ (cfg(any(windows, test)), forbid(unsafe_code)); shell/ (cfg(windows))
      apple/                what only macOS and iOS use: the same pair
      macos/ ios/ android/ web/ linux/    the same pair
      winit/                stand-in for Linux until the native backend (section 11)
      headless/             always compiled, selected at run time
```

`shared/` and `traits/` disappear.

- **Where code lives.** Code of one OS lives in that OS's directory, code of one family in the
  family's, and only code that two or more families use goes to `owner/` or `mapping/`. The names
  say what the modules hold; there is no `common`, `util` or `kit`.
- **`rules` and `shell`.** `rules` is pure translation and compiles on every host in a test build
  (`cfg(any(<os>, test))`) under `forbid(unsafe_code)`; `shell` holds the bindings and the
  `unsafe`, under the OS `cfg`. `rules` names no `shell`, no binding crate and no other OS; the
  compiler checks that on the CI host, where `windows::shell` does not exist.
- **Direction is checked.** `cargo heli xtask module-dag` is extended to `flui-native`: `owner`
  and `mapping` do not depend on `platforms`; `platforms/<a>` does not depend on `platforms/<b>`
  except `macos` and `ios` on `apple`; `rules` does not depend on `shell`.
- **Tests of rules sit beside the rules** as `pub(crate)` modules and run on the Linux CI host.
  `tests/` keeps the conformance table and the recordings of native traces. The 21 files that name
  `shared::*` today move. The Definition of Done in AGENTS.md ("through the public API from
  `tests/`") takes this exception for rule modules.
- **One call point for user code.** The application's and a window's handlers are called through
  one point in `owner/` that releases every lock and borrow first; a replaced handler is dropped
  outside any lock. The six lease helpers (`RunningSessionEndHook`, `CallbackLease`,
  `InputLease`, headless `WindowEventLease`, winit `WindowEventHandlerLease` and
  `ExitPolicyHookLease`) and the hand-written take-and-restore code of macOS, iOS and the Web go.
- **Moves re-key the `globals` entries** (19, keyed by module path), the path-keyed xtask
  allowlists and the `include_bytes!` path of the Android dex.

### 11. Native Linux, then winit goes

A native Linux backend (Wayland and X11) is built after the restructuring, on the live
`platforms/linux` code (display source, logind notices, AT-SPI adapter). The 274-line
`LinuxPlatform` stub is deleted with it. When it runs, winit is removed and CI's Linux jobs move
to the native backend. ADR-0082 §5 kept winit for Linux until a native backend had a second
reason to exist; the reason is now the work itself.

### 12. The rename: two pull requests, core host first

1. `crates/flui-platform` becomes `crates/flui-native`: package, library, paths, crate-qualified
   features (`flui-native/a11y`), `allowed-dependents`, the root `reach.tier.K.forbid` (it forbids
   `flui-native`, and must not forbid the new `flui-platform`, which is tier C and a dependency of
   `flui-runtime`), `globals` and the xtask paths, the nextest filter `package(flui-native)`, the
   weekly workflow's `--exclude flui-native`, `tracing` targets and `RUST_LOG` strings, and the
   worker-thread name `flui-native-bg`.
2. `crates/flui-platform-api` becomes `crates/flui-platform`: `flui_platform_api` to
   `flui_platform`, `SDK_SURFACE` in `tools/xtask`, the `key_vocabulary` generator, the nextest
   `binary_id(flui-platform::compile_fail)` and the feature `flui-platform/serde`.

Between the two, no crate named `flui-platform` exists, so the compiler rejects any use of the
retired name in code and no retired-name check is needed for it. After the second, the name exists
again and means the contract: a string no compiler reads that a branch written before the first
rename carries (`--exclude flui-platform`, a nextest filter) silently aims at the contract. The
second pull request therefore begins by checking that no `flui-platform` or `flui_platform` token
remains outside the archival roots and this ADR, and a branch that merges after the second pull
request applies both maps in order: the first pull request's, then the second's.

The tool is `git mv` and `sd` on whole tokens,
longest match first; each pull request states its exact commands so any branch can repeat them.
Archival `docs/plans` and `docs/research`, the text of this ADR and released entries of
`CHANGELOG.md` are not rewritten. Documents that describe the relation between the two crates are
rewritten by hand, not swapped.

Traps (found with `rg` and `git grep` over the tree at `main` `083b0f75d`):

- trybuild `.stderr` files hold column numbers; they are regenerated with `TRYBUILD=overwrite`,
  never text-replaced. Five of them, in `crates/flui-platform/tests/ui/`, hold
  `flui_platform::traits::` source lines with caret rows. `Cargo.lock` is regenerated by cargo.
- The strings `flui_native_recording` (a recording header key in the 15 files of
  `crates/flui-platform/tests/native_recording/`) and `flui-native-library-*`
  (`crates/flui-hot-reload/tests/scene_ownership.rs:94`) already exist and are not part of the
  rename.
- Strings that stay valid after a missed first rename and then point at the contract:
  `.config/nextest.toml:126` (`package(flui-platform)`), `.github/workflows/weekly.yml:96`
  (`--exclude flui-platform`) and the tier K `forbid` list at root `Cargo.toml:135`. The contract's
  own name sits in `tools/xtask/src/workspace/tiers.rs:53` (`SDK_SURFACE`) and
  `.config/nextest.toml:69` (`binary_id(flui-platform-api::compile_fail)`).
- After the first pull request the contract's names reach `flui-app` as
  `flui_native::WindowOptions` and stay so until `flui-app` takes contract paths (section 9).

One pull request would break `git blame` on paths that exist before and after
(`crates/flui-platform/`). A `cargo xtask rename` tool would be written for one rename.

### 13. `AppLifecycleState` stays in `flui-scheduler`

It is an aggregate of the UI runtime (ADR-0035 §4: lifecycle facts are per presentation, the
runtime aggregates them). Backends publish presentation facts (`WindowExecutionState`,
visibility) and name no scheduler type. Moving it to the contract would put framework policy
(`should_render`, `should_animate`) in a Stable crate. ADR-0035 stands unchanged; the earlier
proposal to supersede its §1 is dropped.

### 14. OS floors stand; Android's `targetSdk` is pinned

[ADR-0244](ADR-0244-os-version-floors-and-gates.md) is right and stands: the lowest floor the
stack allows, version gates as types, no version in the contract. One addition: Android's
`targetSdk` is pinned in `platform-support.toml` and generated into the packaging values like
`minSdk`, where today it is the latest SDK installed on the build machine.

### 15. Accessibility is not optional

The `a11y` Cargo feature is removed from the core host, from `flui-app` and from the facade, and
the AccessKit adapters (UI Automation, NSAccessibility, AT-SPI; ADR-0255 adds Android, iOS and the
Web) compile on every OS, always. Today the feature is off by default: the core host declares
`a11y = ["dep:accesskit_unix", "dep:accesskit_windows", "dep:accesskit_macos"]`
(`crates/flui-platform/Cargo.toml:437`), `flui-app` forwards it and gives the reason that the
AT-SPI adapter brings a D-Bus stack (`crates/flui-app/Cargo.toml:50-56`), and the facade forwards
it again (`Cargo.toml:867`). A product that is otherwise ready therefore ships with a semantics
tree no screen reader can see, because its author did not know a feature existed.

The cost the feature avoids is paid anyway on Linux. The winit stand-in already links `zbus`
(`winit-backend` at `crates/flui-platform/Cargo.toml:441-442`, enabled by `flui-app` on Linux at
`crates/flui-app/Cargo.toml:137`), and the native Linux backend of section 11 needs D-Bus for
logind notices (`crates/flui-platform/src/platforms/linux/host_notices/logind.rs`) and for portals.

### 16. Quality bar

The qualities that `docs/platform-parity.md` lists under "Quality bar" are requirements of the
platform layer, each with the gate or the run that proves it, not wishes. A backend's rows do not
turn ✅ while one of them fails on it.

- **Zero CPU when idle.** Every OS signal is a notification; a timer poll is a defect, not a
  fallback.
- **A panic in user code never unwinds into the OS.** A gate checks that every `extern "C"`
  function and every OS callback passes `panic_boundary`
  (`crates/flui-platform/src/shared/panic_boundary.rs`); an unwind into C is undefined behaviour,
  and a missed boundary is invisible in review.
- **One monotonic timeline** for input, frames and presentation: `EventTime` carries all three.
- **Presentation feedback reaches the frame clock**, from the source each OS has: DXGI frame
  statistics on Windows, `addPresentedHandler` on Apple platforms, `wp_presentation_time` on
  Wayland and frame timelines on Android. The Web has no presentation time
  (`requestAnimationFrame` timestamps mark the start of the frame, not its display), so its cell
  says so rather than claiming one.
- **Colour is in the contract's vocabulary:** HDR, wide gamut and the display's colour profile (a
  surface colour space, headroom, a profile change), not left to each backend.
- **Sandbox-aware file access** (portals, security-scoped bookmarks, the Storage Access Framework,
  AppContainer): the layer never hands out a path the process cannot use.
- **Secure text entry and concealed clipboard entries** are part of text input and the clipboard.
- **Activation follows the OS's focus rules** (`xdg-activation`, the hand-off of the foreground
  right on Windows) and does not force focus.
- **Conformance is executed with assistive technology** (Narrator, VoiceOver, Orca, TalkBack),
  not only by reading one control through UI Automation or AX.
- **Native recordings replay on the headless backend** deterministically and cover IME, lifecycle,
  preferences and displays, not only the pointer.

These are properties of areas already on the closed list (displays, input, text, lifecycle, data
directories). They add no area to it.

### Consequences

- Good, because `flui::platform` loses the items that had no consumer above the backend, which
  costs nothing before the first publication.
- Neutral, because `flui::native::<os>` appears behind the feature `native`.
- Good, because widget authors get the lock, ledger and generation through `flui-sdk`, and the
  contract's Stable surface shrinks by about 1,600 lines of policy and the test doubles.
- Good, because the rules halves of the Windows, macOS, iOS and Android backends run on the Linux
  CI host, which is what the split buys.
- Bad, because the Windows, macOS, iOS and Android shells are compiled but not run by CI, so the
  shells need local runs.
- Bad, because every open branch repeats each rename's commands, and a string no compiler reads (a
  nextest filter, a CI argument, a `RUST_LOG` value, an xtask path) that a branch adds after the
  first rename is found by review only; the second rename then makes it point at the contract.
- Bad, because `flui-widgets` and `flui-app` change their import paths widely (the changes are
  mechanical).
- Bad, because the restructuring moves most of 97,000 lines; the two renames come first, and the
  restructuring follows in pull requests of its own.
- Neutral, because rule tests move into the crate, and AGENTS.md's Definition of Done gets the
  exception stated in section 10.
- Bad, because the documents that map the crates (`docs/crates.md`, `docs/architecture.md`, the
  READMEs and `ARCHITECTURE.md` of both crates) are rewritten.
- Neutral, because text in older ADRs that names `flui-platform` or `flui-platform-api` keeps its
  decisions; the crate names in it follow this ADR's map.
- Bad, because every build links the AccessKit adapter of its OS, and a Linux build links the
  AT-SPI adapter and its D-Bus client whether or not the application asked for accessibility.
- Neutral, because the qualities of section 16 are rows with gates in `docs/platform-parity.md`.
- Bad, because the rows that are ❌ or 🟡 today (zero idle CPU, assistive-technology conformance,
  the `panic_boundary` gate) become work of the series, and a platform is not "ready" while they
  stay so.

### Confirmation

- `cargo heli xtask workspace` (tiers, layers, `allowed-dependents`), `reach` (the contract
  forbids `accesskit`, `tokio`, `tracing` and `parking_lot`; tier K forbids `flui-native` and not
  the contract), `globals` (re-keyed entries), `module-dag` (extended to `flui-native` as in
  section 10) and `checks`.
- `cargo heli xtask cross-typecheck` for Windows, macOS, Android, iOS and wasm32 on every pull
  request that touches a backend or either rename.
- Before the first publication: the API-closure gate of ADR-0089 §6 and `clippy::exhaustive_enums`
  on the contract, with every closed enum carrying its reason.
- The `forbid(unsafe_code)` of a `rules` module holds under its parent's `expect(unsafe_code)`: a
  planted `unsafe` block in a rules module fails the build. This is unverified in the repository
  today and is proven when the first `rules` module lands.
- Each rename: the map re-run on its own output changes nothing; regenerated trybuild snapshots
  are reviewed as diffs. The second rename's opening check finds no `flui-platform` or
  `flui_platform` token outside the archival roots and this ADR.
- No manifest declares an `a11y` feature, and `cargo heli xtask cross-typecheck` builds each OS's
  AccessKit adapter without a flag.
- The panic-boundary gate fails on a planted `extern "C"` function or OS callback that does not
  pass `panic_boundary` (a `--self-test` case, as the other gates have).
- Native behaviour is run per platform and recorded in `docs/platform-parity.md` (a cell is marked
  done only after a run on that platform); a change to a native backend shows its run in the pull
  request.
- Review only, with no gate: the regenerated trybuild snapshots (reviewed as diffs), and a string no
  compiler reads (a nextest filter, a CI argument, a `RUST_LOG` value, an xtask path) that a branch
  adds after the first rename (see Consequences).

## Pros and cons of the options

### 1. The restructuring this record decides

- Good, because the contract takes the name of the path users already write
  (`flui::platform::WindowOptions`), and the core host the name of the code that talks to the OS
  (section 1).
- Good, because one crate for every native backend needs no public seam for the OS crates to reach
  into the core, no `cfg` moved to a facade or feature forwarding, and no extra tier, layer,
  `globals`, reach and tooling entries (section 1).
- Good, because the contract holds only vocabulary and traits that something above the backend
  uses, so no item is a promise nobody tests (section 2).
- Good, because the closed list gives the core host a membership rule, and what the rule leaves
  out is a capability crate (section 3).
- Good, because `Unsupported { reason }` replaces nine spellings of "not here", and the contract
  never carries an OS version (section 4).
- Good, because `clippy::exhaustive_enums`, the API-closure gate and explicit seals make the
  stability of the contract checkable (section 5).
- Good, because the `rules` and `shell` halves let the compiler and `module-dag` keep single-OS
  translation apart from the bindings, and let the rules run on the Linux CI host (section 10).
- Good, because accessibility is always built and the quality bar is a set of requirements with
  gates (sections 15 and 16).
- Bad, because it moves most of the 97,000 lines of the core host and changes import paths widely.
- Bad, because the renames make every open branch repeat commands, and a string no compiler reads
  is found by review only.
- Bad, because a second consumer of an OS backend's internals, or a backend with its own tier,
  cadence, toolchain or licence, reopens the crate-per-OS question (see More information); if the
  boundaries of section 10 hold, extraction is a move.

### 2. Keep `flui-platform-api` and `flui-platform`

- Bad, because `flui-platform-api` is a three-part name against the `flui-<word>` rule, and the
  crate named like the facade path `flui::platform` would be the one holding OS code.

### 3. Name the contract `flui-port`, `flui-contract` or `flui-core`

- Bad, because the crate path and the facade path `flui::platform` diverge; "port" reads as
  porting; `core` is a banned word in the naming rules.

### 4. Name the core host `flui-os` or `flui-backend`

- Bad, because "os" fits Web and headless worse; "backend" already names the GPU backend and
  `TextInputBackend`, and each `platforms/<os>` is itself a backend.

### 5. A crate per OS (winit, GPUI)

- Good, because it serves out-of-tree backends, users of a backend's types without the event loop
  and dependents that need not wait for platform crates, which is why winit and GPUI split
  (section 1).
- Bad, because FLUI has no out-of-tree backend and one dependent, `flui-app`, which needs every
  backend to link: a split cannot shorten that build, only overlap work (reasoned, not measured).
- Bad, because it makes public what the OS crates reach in the core (468 lines in `platforms/`
  name `crate::shared::` or `crate::traits::`).
- Bad, because the `cfg` moves to a facade and to feature forwarding instead of disappearing (70
  lines starting `#[cfg` in winit's facade, 19 lines naming `cfg` in `gpui_platform`).
- Bad, because each crate adds a tier, a layer, `globals`, reach and name-keyed tooling entries
  and a unit in the first publication, and ADR-0041 admits a new crate only for a new layer or a
  proven second consumer.
- Bad, because winit's own split came after more than a year of turning its API into traits,
  while FLUI's seam is closed on purpose.

### 6. Per-OS crates behind a facade

- Bad, because it has the same costs as option 5 plus feature forwarding (Slint's selector reaches
  six crates from one feature).

### 7. A pure `rules` crate plus FFI shells

- Bad, because it is one crate with no layer of its own (ADR-0041), and every rule function is
  `pub` again.
- Bad, because one crate of every OS's rules is `shared/` renamed.

### 8. Keep `shared/` or rename it `backend/`

- Bad, because 45% of `shared/` is single-OS code and `pub #[doc(hidden)]` hides it.
- Bad, because `backend::` beside `platforms::` reads as "the backends".

### 9. Everything in the core host (GPUI's 76-method `Platform`)

- Bad, because services that are permission-gated or hardware-bound would sit in one crate with
  the window loop (section 3); this is the shape the closed list refuses
  (`zed-industries/zed@fae8a35482 crates/gpui/src/platform.rs:363-604`).

### 10. Menus and haptics as capability crates

- Bad, because framework widgets call them under a cross-platform contract and could not name
  their handles.
- Bad, because the Dock menu needs the application delegate.

### 11. A contract that anticipates consumers

- Bad, because an item with no consumer is a promise nobody tests: `WindowEvent`,
  `WindowBackgroundAppearance` and the `OfferTable` family are the cost.

### 12. Its own contract version at once

- Bad, because the contract depends on `flui-foundation` (`links = "flui_train"`); section 5
  enters its own version when `cargo-semver-checks` shows two stable trains.

### 13. A hidden second tier for first-party backends (Slint)

- Bad, because a third-party backend becomes second class and cannot deliver touch or IME.

### 14. A synchronous `Clipboard` with an asynchronous twin (GPUI)

- Bad, because it is two traits for one fact, and the synchronous one cannot be honest on the Web
  or Wayland.

### 15. Keep the lock, ledger and generation in the contract, or move them to `flui-widgets`

- Bad, because it puts one consumer's policy in a Stable crate.
- Bad, because `flui-widgets` is higher than `flui-interaction`, which already drives text input
  (packages read through `flui-sdk` either way).

### 16. `AppLifecycleState` in the contract

- Bad, because it puts framework policy in a Stable crate; backends need only the presentation
  facts they already publish.

### 17. Keep winit as the Linux backend

- Good, because it is the one real backend CI executes today (X11 under Xvfb, Wayland under a
  headless weston) and it costs no new code.
- Bad, because it is a stand-in by design (ADR-0082 §5's own wording) that costs 11,262 lines and
  153 `#[cfg` lines of a path no other OS takes (`crates/flui-platform/src/platforms/winit`).

### 18. One pull request for both renames

- Bad, because it breaks `git blame` on paths that exist before and after.

### 19. A `cargo xtask rename` tool

- Bad, because it is written, tested and kept for one rename.

### 20. `cfg` by OS version, and "latest installed SDK" for `targetSdk`

- Bad, because a literal version in code is what ADR-0244's gates exist to prevent.
- Bad, because the build then depends on the machine.

### 21. `a11y` stays a feature, off by default

- Good, because it keeps the AT-SPI adapter's D-Bus stack out of builds that did not ask for
  accessibility (`crates/flui-app/Cargo.toml:50-56`).
- Bad, because a product that is otherwise ready ships without accessibility when its author did
  not know the feature existed.
- Bad, because the D-Bus cost it avoids is paid by the native Linux backend anyway (section 15).

### 22. Quality after the features

- Bad, because a layer that compiles on six operating systems and polls one of them on a timer is
  the half-built layer this series ends; the qualities of section 16 are requirements, not a later
  pass.

## More information

**Reopening.** Crate per OS is reconsidered if any of these holds: a second consumer of an OS
backend's internals appears (an external embedder once its seam is designed, or a capability crate
that needs the same window-message hooks); `cargo heli build --timings` shows the core host on
`flui-app`'s critical path for a target with one OS half dominating it; a backend gets its own
support tier or release cadence; a backend needs a build toolchain consumers should not install;
a licence differs. If the boundaries of section 10 hold, extraction is a move.

**Revisit when the contract can version apart.** The contract's own version (section 5) is entered
under ADR-0153 option 1 when `cargo-semver-checks` shows the contract stable for two trains in a
row; ADR-0153 keeps the cases and the measurement.

**Sources.** The pins for other projects are in the Sources bullet of the header and cited inline
as `owner/repo@sha path:line`; counts and lines of FLUI's own tree are at `main` `083b0f75d`.
