# ADR-0151: Platform layer boundary, crate names and the home of shared platform vocabulary

- **Status:** Proposed (2026-10-06). Awaiting the owner's approval; nothing in this ADR is
  implemented.
- **Date:** 2026-10-06
- **Supersedes, on acceptance:**
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) §1's item list and §2's crate
  names (the boundary itself stands); [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §1's
  placement of `AppLifecycleState` in `flui-scheduler` (the state machine, the aggregation and
  every other rule stand).
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (tiers, reach),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (types low, instances up),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (SDK surface),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (no upstream types),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (globals),
  [ADR-0152](ADR-0152-capability-seam-revised.md) (capability seam)

## Context

ADR-0082 made `flui-platform-api` the contract crate and kept every OS backend in
`flui-platform`. The boundary was right; what grew on each side of it was not:

- The contract crate, which the facade re-exports whole as `flui::platform`, carries about
  2,000 lines of text-store implementation and containment policy (`LockArbiter`,
  `OwnerCalls`, `CompositionLedger`, `EditGeneration`, `project_ime_event`), two test doubles
  (`InMemoryClipboard`, a 661-line `InMemoryTextStore`), a backend slab (`OfferTable`), Win32
  pixel helpers, OS-specific enum variants (`WindowBackgroundAppearance::Mica*`) and items no
  crate above the backend uses (`PlatformDisplay`, `WindowBounds`, `WindowMode`, `WindowEvent`).
  Its signatures name `ui-events`, `keyboard-types` and `dpi` types, which ADR-0089 forbids.
- The backend crate holds contracts a consumer above it would need (`PlatformCapabilities`,
  `PathPromptOptions`, `PlatformExecutor`), dead code (`LinuxPlatform`, whose every method is
  `unimplemented!()`; the legacy `window.rs` with its raw-pointer `RawWindowHandle`;
  `BackgroundExecutor` and `Task` on tokio, an ADR-0047 residual), public OS types
  (`win32::HWND`, `NSApplication`, `AndroidApp`, `HtmlCanvasElement`,
  `tokio::runtime::Handle`) and modules named `shared` and `traits`, which the naming rules
  forbid.
- System settings have no producer. `AccessibilityFeatures` in `flui-semantics` is written
  and read by nothing (`#[expect(dead_code)]` in `flui-app`), `MediaQueryData::text_scale_factor`
  is always 1.0, no backend reports a preferred-locale list, and `flui-interaction`'s
  `GestureSettings` are constants: nothing calls `for_platform` or `native()`. Two efforts plan
  separate seams for parts of this (a motion setting for animations, a gesture-settings source
  for recognizers).
- `AppLifecycleState` is defined in `flui-scheduler`, its consumer, while its producer is the
  host, which seeds `PlatformToUi::Lifecycle(AppLifecycleState)`. ADR-0082 §1 already calls the
  contract crate's `WindowExecutionState` "the single lifecycle state machine of ADR-0035", so
  the two halves of one vocabulary sit in two crates.
- `flui-platform-api` is a three-part name; the owner's rule is `flui-<word>`.

## Decision

### 1. Two crates, renamed

| Role | Today | New name | Tier / kind | Layer |
|---|---|---|---|---|
| Contract | `flui-platform-api` | **`flui-platform`** | C/1, `stable` | 1 |
| OS backends | `flui-platform` | **`flui-native`** | H/1, `internal` | 3 |

There is no third platform crate: no `-core`, no crate per OS. The contract's crate name now
matches the facade path users already write (`flui::platform::Clipboard`), and the seam reads
without stutter (`flui_platform::Capability`). Both names are free on crates.io (checked
2026-10-06).

The rename is one mechanical pull request produced by a script, backend first
(`flui_platform` → `flui_native`, directory included), then the contract
(`flui_platform_api` → `flui_platform`); the reverse order would merge the two names. The
script also updates the crate names hard-coded in `tools/xtask` (`globals.rs` `PLATFORM` and
`BACKENDS`, `tiers.rs` `SDK_SURFACE`), `allowed-dependents`, the crate-keyed allowlists, and
repository paths in living documents and ADRs. Paths in ADRs are facts and change; no older
ADR's decision text changes, and this ADR is the map from old names to new ones. Archival roots
(`docs/research`, `docs/plans`) are not rewritten.

### 2. What the contract crate holds

Vocabulary and traits that something above the backend uses, and nothing else:

- `window`: `PlatformWindow`, `WindowId`, `WindowOptions`, `WindowAppearance`,
  `WindowExecutionState`, `CursorError`. Methods with no consumer above the backend (`display`,
  `window_bounds`, `set_background_appearance`, `mouse_position`, `is_hovered`) move to
  `HostWindow`.
- `input`: FLUI-owned pointer and keyboard vocabulary (ADR-0089 §4), `ImeEvent`,
  `PlatformTextInput`.
- `text_store`: the store, host and observer traits and their values. Where the lock, ledger
  and owner-call machinery goes is decided after the Win32 text-services work lands; until then
  it stays.
- `clipboard`, `storage`, `data_transfer` (vocabulary only), `haptics` (until it becomes a
  package, ADR-0152).
- `lifecycle`: `AppLifecycleState` (moved from `flui-scheduler`, which re-exports it at its
  current path) next to `WindowExecutionState`.
- `preferences`: `SystemPreferences` (§4).
- `locale`, `target_platform`, `capability` (ADR-0152).

It holds no OS code, no `unsafe`, no test doubles, no backend tables, and no upstream type in a
public signature beyond ADR-0089 §2's exceptions. `InMemoryClipboard` and `InMemoryTextStore`
move to `flui-testing`; `OfferTable`, the pixel helpers, `WindowMode`, `WindowEvent`,
`WindowBounds`, `PlatformDisplay` and the `Mica*`/`Vibrant*` variants move to the backend
crate.

### 3. What the backend crate holds

`Platform`, `OwnerPlatform`, `SharedPlatform`, `PlatformProxy`, `HostWindow`, one module per
backend (`windows`, `macos`, `ios`, `android`, `winit`, `web`, `headless`), the file store, and
the cross-OS mapping rules now in `shared/`, split into modules named for what they map.
OS types in its API are `pub(crate)`; an example that needs one reaches it through a
`#[doc(hidden)]` module. `LinuxPlatform`, `window.rs`, `BackgroundExecutor`, `Task`,
`PlatformExecutor` and `PlatformCapabilities` are deleted (no production consumer). Only
`flui-app` depends on it, as today.

### 4. System settings are one snapshot the backend produces

`flui_platform::SystemPreferences` is a `#[non_exhaustive]` value holding brightness,
contrast, text scale, motion, bold text, the preferred-locale list and gesture timings.
`PlatformWindow::preferences()` reads it and `on_preferences_changed` reports a change. A
backend with no answer from the OS returns `SystemPreferences::default()`, whose values are
documented next to the type. The runtime publishes it through `MediaQuery`, builds
`GestureSettings` from it, and feeds the animation duration scale from it.
`AccessibilityFeatures` is deleted.

One snapshot rather than a seam per consumer, because each backend has one place that reads OS
settings and one notification to subscribe to; separate seams would subscribe three times to
one OS event and answer "no value" three different ways.

### 5. Shared vocabulary lives in the lowest crate that needs it

A type shared by a producer (a backend) and a consumer (scheduler, animation, interaction)
lives in the contract crate; the consumer re-exports it when its path is public. Instances stay
with their owners (ADR-0083).

## Alternatives considered

| Alternative | Why not |
|---|---|
| Keep `flui-platform-api` / `flui-platform` | Violates the two-part rule; the contract keeps a name no other framework uses for this layer |
| Contract `flui-platform`, backends `flui-os` | "os" fits web and headless worse than "native"; a two-letter word is hard to search |
| Contract `flui-platform`, backends `flui-backend` | "backend" already names the GPU backend and `TextInputBackend` in FLUI |
| Contract `flui-port`/`flui-contract`, backends keep `flui-platform` | Smaller rename, but the crate path and the facade path `flui::platform` diverge; "port" reads as "porting" |
| Contract `flui-core` (the winit/masonry/slint norm) | `core` is a banned word in the naming rules |
| One crate per OS backend (winit 0.31) | winit split because it has external backend authors; FLUI has none, and ADR-0082 already rejected it |
| Three settings seams (motion, gesture, rest) | Three subscriptions and three fallbacks for one OS event |
| Keep `AppLifecycleState` in `flui-scheduler`; the platform reports only per-window facts | The host already produces `AppLifecycleState` values; the producer would keep depending on a consumer's type |

## Consequences

- `flui::platform` loses items that were never a promise users could rely on; pre-1.0 this is
  the cheapest moment.
- Every open branch that names `flui_platform` or `flui_platform_api` needs the same script
  after the rename lands; the transition plan gives the instruction.
- `flui-scheduler` gains a normal edge to the contract crate (S → C, layer 2 → 1, allowed).
- The rename is scheduled with the naming sweep, before the publish pipeline, so it ships in
  0.2.0 once.

## Verification

`cargo xtask workspace`, `reach`, `globals`, `checks` (`docs-paths`, `docs-links`, `markers`),
`check-changed`, and `cross-typecheck` for Windows, macOS, Android and iOS on every pull request
of the transition. Win32, AppKit, Android and iOS code is type-checked only unless a pull
request shows a local run.
