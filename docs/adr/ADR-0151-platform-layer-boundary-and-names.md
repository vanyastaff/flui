# ADR-0151: Platform layer boundary, crate names and the home of shared platform vocabulary

- **Status:** Proposed (2026-10-06). Awaiting the owner's approval; nothing in this ADR is
  implemented.
- **Date:** 2026-10-06
- **Supersedes, on acceptance:**
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) §1's item list and §2's crate names (the
  boundary between a contract crate and a backend crate stands);
  [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §1, for the crate that
  defines `AppLifecycleState` only (the state machine, the per-presentation facts, the
  aggregation and every other rule stand).
- **Adopts:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §1–§2 for the contract
  crate, whatever ADR-0089's own status is.
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md),
  [ADR-0152](ADR-0152-capability-seam-revised.md) (capability seam),
  [ADR-0153](ADR-0153-stable-crates-do-not-ride-the-train.md) (Stable crates off the train),
  [ADR-0154](ADR-0154-capability-crates.md) (capability crates)

## Context

ADR-0082 made `flui-platform-api` the contract crate and kept every OS backend in
`flui-platform`. The boundary was right; what grew on each side of it was not.

- **The contract crate**, which the facade re-exports whole as `flui::platform`
  (`src/lib.rs:183`), carries about 2,000 lines of text-store implementation and containment
  policy (`LockArbiter`, `OwnerCalls`, `CompositionLedger`, `EditGeneration`,
  `project_ime_event`), two test doubles (`InMemoryClipboard`, a 730-line `InMemoryTextStore`),
  a backend slab (`OfferTable`), Win32 pixel helpers, a Windows-only enum variant family
  (`WindowBackgroundAppearance::Mica*`), and items nothing above the backend uses
  (`PlatformDisplay`, `WindowBounds`, `WindowMode`, `WindowEvent`, the data-transfer vocabulary).
  `PlatformHaptics` is implemented only by the headless fake. Its signatures name `ui-events`
  types and, through them, `keyboard-types` and `dpi`.
- **The backend crate** holds vocabulary with no consumer above it (`PlatformCapabilities`,
  `PathPromptOptions`, `SessionEndPhase`), a stub (`LinuxPlatform`, whose methods are
  `unimplemented!()` except `name` and `data_transfer`), the legacy `window.rs` with a
  raw-pointer `RawWindowHandle`, public OS types (`win32::HWND`, `NSApplication`, `AndroidApp`,
  `HtmlCanvasElement`, `tokio::runtime::Handle`) and modules named `shared` and `traits`.
  `BackgroundExecutor` and `Task` are live: Win32, macOS and winit use them, and Win32's file
  dialogs return `Task` as ADR-0039 §2 prescribes.
- **System settings have no producer.** `AccessibilityFeatures` (flui-semantics) is written and
  read by nothing; `MediaQueryData::text_scale_factor` is always 1.0; no backend reports a
  preferred-locale list; `GestureSettings` are constants and nothing calls `for_platform` or
  `native()`. The animation work planned a per-window `SystemMotion` and the interaction work a
  `GestureSettingsSource`: two producers for one OS notification.
- **`AppLifecycleState`** is defined in `flui-scheduler`. `flui-app` produces it today, so no
  dependency edge is wrong yet; but ADR-0035's unimplemented signals (Windows minimize, web
  `visibilitychange`, Android pause/resume) will be produced by backends, which must not name a
  scheduler type. ADR-0082 §1 already calls the contract crate's `WindowExecutionState` "the
  single lifecycle state machine of ADR-0035".
- **`flui-platform-api` is a three-part name**; the owner's rule is `flui-<word>`.

## Decision

### 1. Two framework platform crates, renamed in two steps

| Role | Today | New name | Tier / kind | Layer |
|---|---|---|---|---|
| Contract | `flui-platform-api` | **`flui-platform`** | C/1, `stable` | 1 |
| Core host (OS backends) | `flui-platform` | **`flui-native`** | H/1, `internal` | 3 |

Optional capabilities with their own OS code are a third class of crate, decided separately in
ADR-0154. There is no `-core` crate and no crate per OS for the framework's own platform code.

The contract's crate name matches the facade path users already write
(`flui::platform::Clipboard`). Both names are free on crates.io (checked 2026-10-06).

Because `flui-platform` changes meaning, the rename never uses one name for two crates in one
step:

1. **Step 1** renames the backend to `flui-native` and adds a retired-name check that refuses
   `flui-platform`/`flui_platform` outside archival roots.
2. **Step 2**, once every open branch has merged step 1, renames `flui-platform-api` to
   `flui-platform` and lifts the check.

Each step is one pull request produced by a script, with no hand edits. The script matches whole
crate tokens (`flui-platform` never matches inside `flui-platform-api`), takes
`--changed-since <merge-base>` so a branch rewrites only its own lines, and covers manifests,
the root reach tables (`reach.tier.*.forbid`), `reach-forbid`/`reach-exceptions`,
`allowed-dependents`, the names hard-coded in `tools/xtask` and its fixtures, the crate-keyed
allowlists, and every mention in living documents and ADRs. Old ADRs keep their decisions; their
crate names change mechanically with this ADR as the map. Archival roots (`docs/research`,
`docs/plans`) are not rewritten. Re-running a step's own map on its output is empty.

### 2. What the contract crate holds

Vocabulary and traits that something above the backend uses, and nothing else:

- `window`: `PlatformWindow`, `WindowId`, `WindowOptions`, `WindowAppearance`,
  `WindowExecutionState`, `CursorError`. Methods no consumer above the backend calls (`display`,
  `window_bounds`, `set_background_appearance`, `mouse_position`, `is_hovered`) move to
  `HostWindow`.
- `input`: FLUI-owned pointer and keyboard vocabulary (ADR-0089 §4, owned by the
  pointer-vocabulary work), `ImeEvent`, `PlatformTextInput`.
- `text_store`: the store, host and observer traits and their values. Where the lock, ledger and
  owner-call machinery goes is decided after the Win32 text-services work lands (ADR-0142 pins it
  here until then).
- `clipboard`, `storage`, `locale`, `target_platform`.
- `lifecycle`: `AppLifecycleState`, moved from `flui-scheduler` and made `#[non_exhaustive]`;
  the scheduler re-exports it at its current path and keeps its frame policy
  (`should_render`, `should_animate`) as its own extension trait.
- `preferences`: `SystemPreferences` (§4).
- `capability`: the seam of ADR-0152.

It holds no OS code, no `unsafe`, no test doubles and no backend tables, and no public signature
names an upstream type beyond ADR-0089 §2's exceptions. Before the first publication:
`PlatformHaptics`, `haptics()` and `HapticFeedback` leave it (there is no real backend; haptics
returns as a capability crate under ADR-0154); `InMemoryClipboard` and `InMemoryTextStore` move
to `flui-testing`; `OfferTable`, the data-transfer vocabulary, the pixel helpers, `WindowMode`,
`WindowEvent`, `WindowBounds`, `PlatformDisplay` and `WindowBackgroundAppearance` move to the
backend crate. Each comes back to the contract with its first consumer above the backend.

### 3. What the core host holds

`Platform`, `OwnerPlatform`, `SharedPlatform`, `PlatformProxy`, `HostWindow`, one module per
backend (`windows`, `macos`, `ios`, `android`, `winit`, `web`, `headless`), the file store, the
AT-SPI adapter, and the cross-OS mapping rules now in `shared/`, split into modules named for what
they map. Its OS integration is a **closed list**: windows, input, text input, accessibility,
clipboard, lifecycle, session end, system preferences, surfaces. Anything else is a capability
crate (ADR-0154). Only `flui-app` depends on it.

`LinuxPlatform`, `window.rs` and `PlatformCapabilities` are deleted. `BackgroundExecutor`,
`Task` and the file prompts stay until a capability crate takes over the Win32 dialog; the ADR
that moves them supersedes ADR-0039 §2.

### 4. System preferences: one host source, each consumer its own representation

`flui_platform::SystemPreferences` holds text scale, contrast, bold text, motion
(`NoPreference`, `Reduce`, `Scaled(DurationScale)`), the preferred-locale list and gesture
preferences (double-click interval, double-click area and drag area as logical `Size` per
ADR-0098, long-press timeout). Fields are private; backends build values through a builder whose
setters validate (non-finite or negative values are an `InvalidPreference` error).

- **One producer per host.** `Platform::preferences()` and one `on_preferences_changed`
  subscription in the core host. `flui-app` seeds every realm with the current value at
  construction, so it exists before the first window, and forwards each change as one typed
  host operation. Brightness stays a per-window `WindowAppearance` because platforms let a window
  override it.
- **Consumers do not use it directly in their logic.** `flui-interaction` builds its
  `GestureSettings` from it; animations build their motion policy from it; `MediaQuery` exposes
  what widgets read. This replaces the planned `GestureSettingsSource` capability and the
  per-window `SystemMotion` methods.
- **Application policy over the OS value lives in the framework**, in the realm (for example
  "follow the system / always reduce / never reduce" motion), never in the contract.
- `AccessibilityFeatures` is deleted.

### 5. Shared vocabulary lives in the lowest crate that needs it

A type shared by a producer (a backend) and a consumer (scheduler, animation, interaction)
lives in the contract crate; the consumer re-exports it when its path is public. Instances stay
with their owners (ADR-0083).

## Alternatives considered

| Alternative | Why not |
|---|---|
| Keep `flui-platform-api` / `flui-platform` | Violates the two-part rule |
| Rename both in one step | The name `flui-platform` would denote two crates in one map: a re-run or a branch rebase rewrites the new contract into `flui-native` |
| Contract `flui-port`/`flui-contract`, backend keeps `flui-platform` | One step, but the crate path and the facade path `flui::platform` diverge; "port" reads as "porting" |
| Backend named `flui-os` or `flui-backend` | "os" fits web and headless worse; "backend" already names the GPU backend and `TextInputBackend` |
| Contract named `flui-core` (the winit/masonry/slint norm) | `core` is a banned word in the naming rules |
| Per-window preference producers (one per setting) | One OS notification per setting per window; no value before the first window; each consumer invents its own fallback |
| Keep `AppLifecycleState` in `flui-scheduler` | Backends that produce the missing ADR-0035 signals would have to name a scheduler type |

## Consequences

- `flui::platform` loses items that were never backed by a real implementation; before the first
  publication this costs nothing.
- Every open branch runs the script after each rename step; the transition plan gives the
  instruction and the dates.
- `flui-scheduler` gains a normal edge to the contract crate (S → C, layer 2 → 1, allowed).
- The animation and interaction specs replace their planned producers with consumers of
  `SystemPreferences`.

## Verification

`cargo xtask workspace`, `reach`, `globals`, `checks`, `check-changed`, and `cross-typecheck` for
Windows, macOS, Android and iOS on every pull request of the transition. Win32, AppKit, Android
and iOS code is type-checked only unless a pull request shows a local run.
