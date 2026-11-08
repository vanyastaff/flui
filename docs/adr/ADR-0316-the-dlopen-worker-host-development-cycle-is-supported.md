# ADR-0316: The dlopen worker-host development cycle is supported

- **Status:** Accepted (2026-11-07)
- **Date:** 2026-11-07
- **Supersedes:** [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) §7 only
  (the dlopen path is a supported development cycle, not temporary scaffolding whose exit is a code
  patcher; ADR-0311 §1–§6 are unchanged)
- **Related:** [ADR-0097](ADR-0097-no-process-global-state-gate.md) §2 (the `image` grant class and
  the remaining per-loop registry exits); [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md)
  (the loader's standing reach grants); [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the
  rejected Subsecond spike, retained as historical evidence rather than a live plan);
  [ADR-0108](ADR-0108-plugin-scene-render-callback-safety.md) and
  [ADR-0111](ADR-0111-worker-image-lifetime.md) (scene and worker image lifetime);
  [ADR-0315](ADR-0315-first-party-packages-remain-train-aligned-facade-features.md) (facade
  aggregation, unchanged)

## Context and problem statement

The worker-host reload loop preserves application state across logic edits on desktop and iOS, and
the Android scene loop reloads a scene plugin without restarting its host. Both are useful
development workflows today. ADR-0311 §7 nevertheless treated them as debt to delete when a code
patcher arrived, while ADR-0094's rejected Subsecond proposal supplied the patcher's return
condition. That makes a working development feature contingent on a different, unaccepted tool.

The dlopen design has real costs: a generated application has host, worker and shared-types crates;
each dynamically loaded image has owner-thread state and a lifetime contract; the loaders need
narrow OS dependencies; and several worker registries are still process-global rather than owned
by an event loop. Which of those costs are the supported design, which are standing permissions,
and which remain debt with a concrete ownership exit?

## Decision drivers

- A development workflow that works is not made temporary merely because another reload technology
  may appear later.
- Image isolation, not an eventual code patcher, justifies the three-crate application shape.
- The host names no reload tool, and official packages build on `flui-sdk` (ADR-0311 §1–§6,
  ADR-0088).
- A dynamically loaded image can carry owner-thread state without making that state a process-wide
  resource or sharing it between UI runtimes.
- Loader OS dependencies are different from process-global worker registries: the former are
  intrinsic to the supported loaders, while the latter still need per-loop ownership and routing.
- A rejected spike is evidence for evaluating a future proposal, not an accepted exit condition.

## Considered options

1. Support the dlopen worker-host and Android scene development cycles, grant their intrinsic
   image and loader costs, and keep only the named per-loop registries as debt.
2. Keep the cycles only until a code patcher arrives, with every associated manifest entry exiting
   through that patcher.
3. Delete the dlopen cycles and wait for a code patcher before offering in-process reload again.

## Decision

Chosen option: 1, because it records the development workflow the project supports and isolates its
actual remaining debt. A future code patcher is an independent proposal, not the presumed
replacement for this cycle.

### 1. The dynamic-image development cycle is supported

The dlopen worker-host cycle on Windows, macOS, Linux and iOS, and the scene-plugin cycle on
Android, are supported development-only workflows. They are not temporary paths toward a code
patcher. Their Rust-ABI admission, image lifetime and scene callback obligations remain those of
ADR-0108 and ADR-0111; this record does not turn the plugin ABI into a stable production plugin
interface.

The host, worker and shared-types crates of a generated worker-host application are the deliberate
cost of isolating reloadable code and its shared data layout. That three-crate shape is not waiting
for permission to collapse into one crate. Web has no `dlopen` worker path: its development loop
rebuilds and reloads the page, losing page state, rather than promising state-preserving widget
reload.

A future code patcher requires its own ADR and evidence. Accepting one does not automatically delete
these workflows, revoke this record's grants, or change ADR-0097's grants. Removal or replacement
of any supported path or permission must be decided explicitly by that later record.

### 2. ADR-0311's application and package boundaries are unchanged

ADR-0311 §1–§6 retains its full force:

- the application installs the development hook and scene factories, and each event loop owns the
  instances it builds on its owner thread;
- `flui-app` and `flui-runtime` do not depend on `flui-hot-reload`; the host reaches the tool only
  through the installed hooks;
- the four internal-crate `edge-exceptions` to `flui-foundation`, `flui-layer`, `flui-rendering`
  and `flui-view` still exit through ADR-0311 §6 as the plugin ABI moves into `flui_sdk::hooks` and
  the package moves under `packages/`;
- `flui_view::__runtime` is not a public SDK surface.

ADR-0315's facade aggregation is independent of this record and is not narrowed by it.

### 3. Loader reach is a standing grant

`flui-hot-reload`'s `reach-exceptions` for `windows` and `android_log-sys` use
`grant = "ADR-0316"`. They admit the OS libraries the supported loaders themselves need. They are
not exits through a code patcher and do not permit unrelated OS dependencies in the package.

The four internal-crate edge exceptions remain temporary under §2; this section changes only the
two loader reach entries.

### 4. Image-local state is standing; process-global routing remains debt

ADR-0097 §2 gains the `image` class: **thread-local state whose lifetime is one dynamically loaded
image/owner thread in it, never shared between UI runtimes; destructor suppression such as
`ManuallyDrop` is part of safe image lifetime.** It is not the `process` class, which mirrors one
resource of the whole process.

The globals gate continues to require `grant = "ADR-0097"` for standing global-state grants. The
current entries are:

| Item | Grant class |
|---|---|
| `plugin::app_plugin!::__FLUI_APP_NEXT_THREAD_TOKEN` | `counter` |
| `plugin::app_plugin!::__FLUI_APP_PINNED_THREAD` | `immutable` |
| `plugin::app_plugin!::__FLUI_APP_STATE` | `image` |
| `flui-view`'s `key::registry::REGISTRY_STACK` | `image` |

The following entries remain temporary with `exit = "ADR-0316"`:

- `dispatch::REQUEST_REBUILD`, until each event loop owns rebuild-request routing;
- `worker::REGISTRATION_SESSION`, until each event loop owns worker registration;
- `worker::WORKER_BUILDS`, until each event loop owns its worker build table;
- `flui-view`'s `key::registry::TEST_REGISTRY`, until the legacy fixture lane migrates to the
  production scoped registry activation.

These exits call for per-loop ownership and routing, not deletion of the supported dynamic-image
cycle. The four internal-crate edge exceptions continue to name ADR-0311 §6 instead.

### 5. Reload workflows do not expose an unused strategy descriptor

The unused public `flui_hot_reload::ReloadStrategy` enum is removed from the package's documented
API. The shared `flui_hot_reload::strategy::{env, timing}` constants remain.

The CLI's `ReloadStrategy` trait is a separate, live orchestration mechanism. It keeps its name and
is not the removed package enum.

### Consequences

- Good, because desktop/iOS worker reload and Android scene reload have an explicit supported
  contract instead of an exit tied to an unaccepted tool.
- Good, because standing loader and image-lifetime costs are distinguishable from the four
  registries that still need per-loop ownership.
- Good, because the public workflow documentation no longer presents an unused enum as the reload
  API, while the CLI's actual orchestration trait remains intact.
- Bad, because worker-host applications retain the three-crate shape and its build orchestration,
  and admitted worker images retain the memory and file-lifetime costs of ADR-0111.
- Bad, because per-loop rebuild and worker registries, and the legacy test registry, still need
  migrations with regression tests.
- Neutral, because a future code patcher may coexist with or replace part of this cycle only through
  its own accepted decision; neither outcome is assumed here.

### Confirmation

- `cargo xtask reach` accepts `windows` and `android_log-sys` only through the two
  `grant = "ADR-0316"` entries and continues to report any other forbidden reach.
- `cargo xtask workspace` keeps the four internal-crate `edge-exceptions` on `exit = "ADR-0311"`;
  their removal belongs to the SDK hooks and package move of ADR-0311 §6.
- `cargo xtask globals` accepts `image` only with `grant = "ADR-0097"`. The counter, immutable and
  two image entries in the table keep their stated classes, and each of the four temporary registry
  entries keeps `exit = "ADR-0316"` until its ownership migration lands.
- Package surface checks contain no public `ReloadStrategy` enum, while `strategy::{env, timing}`
  remains available. CLI orchestration continues to compile against its separate `ReloadStrategy`
  trait.
- `docs/gaps.md` lists the per-loop registry work and the legacy fixture migration, not deletion of
  the supported cycle or a wait for a code patcher.

## Pros and cons of the options

### 1. Supported dynamic-image cycles with explicit standing grants (chosen)

- Good, because the documentation and gates describe the workflow the project actually maintains.
- Good, because intrinsic image isolation costs no longer masquerade as a patcher migration plan.
- Bad, because the project continues to support the three-crate shape, dynamic-image safety rules
  and platform loaders.

### 2. Temporary cycles whose exit is a code patcher

- Good, because every exception would superficially point at one future replacement.
- Bad, because an unaccepted tool would determine the lifetime of working development features and
  obscure the difference between standing loader costs and per-loop registry debt.
- Bad, because accepting a patcher would silently revoke permissions without deciding whether the
  dynamic-image workflows should actually disappear.

### 3. Delete the dynamic-image cycles

- Good, because the three-crate template, image-local grants and loader reach would eventually
  disappear.
- Bad, because desktop/iOS state-preserving reload and Android scene reload would be removed before
  a replacement existed.

## More information

ADR-0094 remains a rejected historical spike. Its Windows failure, patch-reach limits and static or
thread-local findings can inform a future proposal, but they are not a scheduled return condition.
ADR-0311 §1–§6 and ADR-0315 remain the authorities for the hook boundary and facade aggregation
respectively.
