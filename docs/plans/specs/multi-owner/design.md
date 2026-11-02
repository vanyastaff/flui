# Multi-owner UI runtime design

- **Status:** Approved by the owner (2026-11-01); implementation starts with headless evidence
  and native Win32 loop/TSF qualification. Dedicated production runners are not implemented.
- **Contract:** [requirements.md](requirements.md). **Work:** [tasks.md](tasks.md).
- **Decision:** [ADR-0284](../../../adr/ADR-0284-ui-runtimes-may-have-dedicated-owners.md).

## Current producers and consumers

At base `bb618a7f7f1510ff11093109094ba02c937d1970`, `runner/host.rs` resolves the
thread-local `APP_RUNTIME` for platform access, storage, fonts, executors and owner wakes.
`AppRuntime` holds `InstalledHost`, renderer registrations and shared engine services.
`InstalledHost` combines logical `OwnerHost`, `NativeBindings`, display state and
`OwnerEffects`. These modules are under `crates/flui-app/src/app/`.

Three existing synchronous contracts constrain placement:

| Production contract | Source at the base | Consequence |
|---|---|---|
| Native input returns `DispatchEventResult` before defaults execute | `crates/flui-platform/src/input.rs:33`; `crates/flui-native/src/input/windows/mod.rs:671,723` | Neither provisional reentrant result can stand in for arbitrary remote dispatch. |
| TSF grants a local store session around `OnLockGranted`; `SetText` returns the actual edit | `crates/flui-native/src/text/windows/tsf/document.rs:196,423,569`; `crates/flui-platform/src/text_store/store.rs:21` | A snapshot plus delayed edits cannot preserve the current pull contract. |
| `FrameSink::submit` classifies actual raster completion | `crates/flui-runtime/src/sink.rs:42,102`; `crates/flui-app/src/app/raster_lane.rs:337,370` | Enqueue is not `Presented`, `NoPresent` or `Retry`. The frame transaction consumes input/haptic state based on the real outcome. |

The initial native topology therefore colocates a dedicated Win32 runtime with a fresh
native loop and its presentation resources. It does not split these three call paths.
`WindowsLoop` already has per-thread class state, a thread-scoped COM apartment and registration
that accepts the same host type (`crates/flui-native/src/owner/windows.rs:131,944,1049`).
That is source evidence for feasibility, not proof that app startup, services or quit already
work across loops. The native qualification task must test those actual consumers.

## Ownership and placement

One application coordinator controls membership, receipts, process services and final exit.
It resides on the initial application owner. Each dedicated Win32 owner constructs and owns
one native loop, one UI runtime and that runtime's windows, text services and raster resources.
Headless supplies an owner loop through the testing kit. No live local value migrates.

| Owner | State | Allowed crossings |
|---|---|---|
| Dedicated Win32 owner | `OwnerPlatform`, HWND/native handlers, local `OwnerHost`/`UiRuntime`, views/signals/futures, TSF store and grants, frame drivers/renderers | Typed owned app commands, service requests, versioned document messages, receipts and weak wakes |
| Initial owner | Existing local runtimes and native loop; the same local contracts as today | The same addressed coordinator protocol as dedicated owners |
| Application coordinator | Run identity, membership revision, launch/quit receipts, shared service lifetime, exit policy | Bounded messages and independently transferable services; no borrowed UI/native objects or arbitrary UI callbacks |

Keep logical runtime membership local to each `OwnerHost`. Split application-run responsibilities
out of `AppRuntime` without making all of it shared. TLS remains a local entry mechanism;
callbacks capture the endpoint for their owner rather than resolving foreign TLS. A typed
`OwnerRoute` selects local admission or a bounded remote endpoint for coordinator/app operations.
Native callback dispatch always ends on the window's own owner.

`FrameBinding` currently contains only `PresentationAddress`; it is an address token, not proof
that the caller may execute another owner's `FrameDriver`. The driver, native window and sink
remain local. Do not add an unsafe transfer implementation or infer authority from a thread ID.

The new owner bootstrap creates native loop resources on their destination; it does not call
full `Application::run` again. Process identity/logging, application services, worker pools,
storage roots and the driver endpoint are initialized once per application. Closing a worker's
last window may stop that loop only after local obligations settle; it cannot invoke global
`on_quit`, stop shared services or terminate another owner's loop.

## Factory, configuration and capability policy

Keep `Application::new`, root-taking APIs, `open_window` and both `WindowPolicy` variants at
their existing local bounds and behavior. In particular `open_window` currently requires
`V: View + Clone + 'static` and refuses rendered `Shared` content; the bare secondary-window
path's `Shared` behavior is not extended by this feature. The new dedicated signature retains
that `Clone` bound on its output, with `Send` only on the construction factory.

The entry is called from any registered owner of a live application and reserves a coordinator
launch ticket. Unsupported host or exhausted capacity refuses before factory invocation.
`AppHandle` remains closed command ingress; factories are handed to the dedicated bootstrap,
not stored as arbitrary callbacks in that handle. The factory is invoked exactly once after
the destination loop has initialized successfully. It returns a root and local
`RuntimeWindowConfig`; the output and its cloning/destruction stay on that owner.

The new config has private fields and no application-wide settings:

| Current `AppConfig` field group | Dedicated disposition |
|---|---|
| `application_identity`, `diagnostics_profile`, `exit_policy`, `executors`, `services`, `storage_dir`, `memory_reclaimers` | Coordinator-owned. Absent from new config; owner resources register with existing global reclaim/exit mechanisms. |
| `title`, `size`, `min_size`, `max_size`, `position`, `mode`, `resizable`, `decorations`, `os_options`, `frame_latency`, `input_latency`, `surface_color`, `placement` | Constructed and used on destination native/UI owner; placement is presentation-addressed. |
| `motion_override`, `show_performance_overlay`, `pointer_resampling`, `frame_failure_detail` | Local policy. Failure-detail privacy cannot relax the application policy. |
| `frame_failure_handler`, `input_tap`, `error_view_builder`, `close_request_handler` | Destination-local hooks, never transferred after construction. |
| `dev_reload`, `dev_agent` | Owner-local attachment to the application's coordinated tool service. No second process listener, shared `Rc` hook or duplicate agent server. |

Existing `AppConfig` public fields stay intact; reuse internal option conversion. New APIs
are admitted only with their production dedicated-launch consumer and facade/SemVer checks.
The factory's named transfer-position exception needs acceptance before changing any allowlist;
count actual facade/SDK rustdoc positions, not a guessed source count. No blanket reseed.

Capability policy must not silently change between owners. Today `CapabilityRegistry` contains
`Rc` providers and each runtime inherits the app registry; it cannot be sent to a worker.
The coordinator supplies an owned manifest of application registrations (capability marker,
name, provider tier and registry generation), without the providers. The destination factory
explicitly recreates plugin/override registrations using captured transferable configuration;
local built-ins come from its own native host. Preserve override > built-in > plugin precedence
and existing duplicate/shared-name errors. The factory opts into reconstructing each application
registration; merely falling back to a native clipboard is not equivalent to an app override.

Before publishing a window, validate complete registration coverage against that manifest.
Missing reconstruction returns a typed launch refusal naming the capability/tier. If the
application's registry generation changed during launch, refuse and clean up that launch;
do not silently apply an old policy or invoke its one-shot factory again. Marker coverage is
not proof of policy equivalence: the production example must test a distinguishable app
override in both local and dedicated windows. Provide no implicit cross-thread RPC wrapper
for arbitrary user providers. A provider that cannot be recreated keeps that launch unavailable.

Normal `CapabilityRegistry::acquire` remains context-dependent: `Unsupported` from an optional
provider is returned unchanged to the consumer, not elevated to launch failure. Registration
conflicts, omitted app policy or inability to initialize the required native loop are launch
errors. No eager acquisition sweep is allowed to run providers outside their lifecycle context.

## Addressing, transport and bounded work

Routes carry a typed application-run identity, owner incarnation, `UiRuntimeId` and, where
needed, `PresentationAddress`. Operations add launch/quit ticket, request sequence, document
revision or device epoch. Use existing identity machinery where sufficient. Exhaustion is
a typed admission failure; thread IDs are diagnostics only.

The cross-owner vocabulary is closed owned app/control data. It excludes local `RuntimeTask`
closures, `Rc`, text grants, native pointers and frame execution authority. Native key/default
handling, pointer dispatch, TSF callbacks, renderer pumping and resize remain owner-local.
No remote enqueue result pretends to be their synchronous return value. Shared-document
messages explicitly define revision/order; no cross-owner shared signal graph is implied.

| Resource | Initial design bound | Overload contract |
|---|---:|---|
| Dedicated owners and launch receipts | 8 owners; 16 outstanding launch receipts per app | Starting and retiring owners count; refuse before accepting. Limits belong to explicit application configuration. |
| Ordinary app/document commands | 256 entries and 4 MiB retained payload per owner | Return typed full/oversize with rejected value before accepting. This is not an extra queue for synchronous native input. |
| Control obligations | Reserved state per admitted owner/presentation and receipt | Retirement, launch completion, quit consultation and recovery cannot be displaced by ordinary traffic. |
| Completion slots | One per admitted request | Reserve before acceptance; dropping receipt abandons observation only. |
| Raster scenes | Existing local mailbox limits | Preserve engine damage/generation/recovery behavior; no new remote frame transport in this scope. |

Bounds account for retained shared backing buffers and temporary copies, not only envelope
size. Check before allocation/copy. These limits are proposed defaults to qualify, not measured
optima. Control bypass cannot erase previously accepted obligations. Enqueue commits before
wake; missing/replaced/panicking hooks retain a wake obligation. Failure either refuses before
acceptance or settles the reserved receipt. Drop rejected/replaced payloads and invoke wake,
tracing or user callbacks only after guards are released.

## Native input, TSF and frame contracts stay local

Each Win32 owner enters its own STA, creates its HWNDs there, installs `TextServices` there,
and drives its local UI and frame transaction there. It releases COM/native resources there
before thread exit. Register the same host's window-class hooks; do not register an incompatible
host type. The existing `ApartmentHold` off-thread leak defense is not a valid retirement path.

Native key handling uses the real UI `DispatchEventResult` before `DefWindowProcW` or custom
defaults. Preserve ADR-0207's reentrant `DEFERRED_UNHANDLED` rule only in its existing case;
neither deferred constant is a universal remote answer. Test prevented and allowed system
shortcuts, ordinary input, focus movement and input-triggered close under two-owner load.

TSF keeps the field's `Rc<dyn TextStore>` and local grant callbacks. `RequestLock` may defer
the grant; once `OnLockGranted` runs, its reads/edits observe the actual session, and `SetText`
returns its actual edit result. Preserve commit-gate ordering, one undo step/notification,
formatter behavior and coherent geometry. No snapshot replica or asynchronous text-store
rewrite is introduced. Existing `TextStoreHost` capability refusal is not broadened as an
excuse to turn working pull input into `WM_CHAR` fallback for dedicated windows.

Admission freshness and accepted custody are different. A new operation for stale focus or
an unrelated document is refused. An already accepted deferred composition completion retains
its original store: focus movement, detach or host shutdown does not erase that obligation
or redirect it to current focus. The original store may still require commit-in-place under
ADR-0135/0142. Test new-request refusal and old accepted completion separately, including after
shutdown, and retain the original edit/close hold until its actual settlement.

Inline raster returns the real `SubmitVerdict`. Do not report queued work as `Presented`
(would falsely release frame-aligned haptics and consume input epochs), `NoPresent` (would
claim completion), or `Retry` while also executing an accepted frame later. Resource-loss
and surface-generation recovery stay on the owner that holds the lane; addressed global retry
fans out to each owner. Hardware presentation feedback remains a distinct observation.

A later split-native/UI host proposal must solve all three contracts explicitly. It needs
separate input-default, text-authority and frame-completion decisions; this spec neither
authorizes those changes nor makes other platforms depend on an invented remote bridge.

## Installation, quit and failure

| State | Obligation and transition |
|---|---|
| Reserved | Coordinator reserves receipt/owner slot and records membership revision. Factory still owned by caller/bootstrap; spawn failure refuses and releases reservation. |
| Starting | Destination initializes loop/STA, invokes factory once, validates capability policy, creates hidden window and mounts root. Any failure tears down all local partial resources before completing failure. |
| Mounted | Local native/logical routes and coordinator membership are committed before reveal/receipt success. Success means mounted/wired/redraw requested, not presented/focused. A pending close can prevent reveal. |
| Retiring | Fence owner admission, settle accepted operations, flush owner persistence, release native/UI resources on the owner, then end that loop. |
| Retired | Thread completion acknowledged; coordinator releases membership/slot. Only final application exit retires process services/pools. |

Before successful spawn rejected factory captures may drop on the caller; after transfer they
drop on the destination even if cancellation prevents invocation. Root `Clone`, constructor,
provider registration, callbacks and cleanup can panic; preserve the first failure and settle
the receipt after local cleanup. Global failure aggregation never formats arbitrary payloads
under coordinator locks. ADR-0258 and ADR-0281 terminal/unwind behavior still applies.

Global quit has a typed round and three outcomes: unrecorded veto, handler failure or
`stay_open` cancels; recorded holds defer; release/discard or an independently closed
presentation consents. A newer quit round invalidates old responses to the coordinator, but
does not erase the underlying accepted close obligations. Only coordinator-initiated global
retirement waits for all consent; independently accepted window closes may finish meanwhile.

Consultation records a membership revision, including reserved/starting launches. New launches
may be admitted while consulting, as current reentrant local APIs allow; they change that
revision. A starting launch must fail/retire or publish and be consulted before quit can
commit. Newly published presentations join the current round; existing consent need not be
asked twice unless a new round begins. On the coordinator, validate the revision, absence of
unsettled launches and full consent, then fence admission and commit in the same state change.
If membership changed, continue consultation; never retire an unconsulted new owner. A race
at the commit boundary either owns a reserved slot in that revision or receives closed
admission. Bound consultation work per turn and preserve continuation debt.

Each owner receives its retirement ticket and acknowledges only after its local loop resources
are released. The coordinator remains alive to service completion; joins only already-finished
threads outside OS callbacks. Windows session-end deadlines are separate from voluntary quit
and retain the existing forced-end contract; do not wait for arbitrary user veto beyond an OS
deadline. A stuck callback cannot be killed safely: report it, retain custody and do not claim
successful shutdown by detaching the thread. Explicit process termination is not a recovery pass.

## Runner and consumer migration inventory

| Existing consumer | Required migration | Acceptance |
|---|---|---|
| `app/runtime.rs`, `runner/host.rs`, `installed_host.rs`, `native_bindings.rs` | Extract run coordinator; keep logical/native bindings local per owner. Inject shared services instead of resolving process startup anew per TLS slot. | MO1, MO5, MO9, MO13 |
| `owner_dispatch.rs`, `window_routes.rs`, `window_install.rs`, `main_window.rs`, `secondary_window.rs` | Preserve local native dispatch; add addressed cross-owner app/control admission and dedicated factory handshake. Keep current rendered `Shared` refusal. | MO2, MO3, MO5, MO6 |
| Win32 owner/window/input/text/accessibility paths | Construct a fresh same-host loop and STA per dedicated runtime; local callbacks, TSF and UIA; retire loop resources on that thread. | MO4, MO7, MO10; actual native matrix |
| `desktop.rs`, `frame_driver.rs`, `frame_pacing.rs`, `window_renderer.rs`, `surface_lifecycle.rs`, `raster_lane.rs` | Instantiate existing local frame resources per owning loop; route app-wide requests by address. Do not replace synchronous verdict with enqueue. | MO8; input epoch/haptic and pixel checks |
| `device_recovery.rs`, `gpu_loss.rs`, `native_retirement.rs`, `terminal.rs`, `application_control.rs` | Distinguish per-owner termination from app termination; coordinated global retry/quit, recorded holds, membership revalidation and final failure custody. | MO8–MO10 |
| `fonts.rs`, `display_host.rs`, `native_text_sizing.rs`, capability setup | Share app service inputs through existing transferable services; native subscriptions and local providers are per owner. Validate registration manifest and distinguish optional refusal. | MO7, MO13 |
| `Application`, facade/SDK, driver/devtools, examples/packages | Keep current entry bounds; expose dedicated factory with real example and typed refusal. Route one app tool service to each local attachment. | MO2, MO11, MO12, MO14 |
| AppKit, native Wayland/X11, winit, `ios.rs`, `session_controller.rs`, `android.rs`, `web.rs` | Retain local native/UI placement and platform lifecycle; adapt coordinator seam only where shared app logic changes. Dedicated placement unavailable pending separate topology decision and actual-host evidence. | MO2, MO11; shared regressions and explicit refusal |
| `flui-testing` and native testing kits | Shared coordinator contracts and deterministic owner controls; actual Win32 loop/input/TSF scenarios, no substitute production runtime. | MO1, MO4–MO12 |

Remove obsolete process-wide ownership assumptions only after real consumers use the
coordinator. Preserve valid local dispatch and Web direct raster; do not delete them as
"legacy remote bypasses". No GPU-sharing redesign, remote text protocol or new rendered-shared
presentation behavior is included. Record every optional-feature/tooling caller in the final
migration check; a missing device is a verification limit, not a reason to omit its path.

## Verification and status

Requirements/tasks separate future tests, executed document checks, historical headless
evidence and missing native qualification. Native input source must distinguish OS input
from framework injection. Deterministic held-owner tests prove CPU independence; native tests
prove default handling, TSF grants, loop affinity, frame outcomes and lifecycle separately.
Reusable controls live in testing kits. Every new regression must fail under its meaningful
counterfactual, without modifying unrelated worktrees.

Accepting the proposal is distinct from enabling a host. Win32 loop colocation must pass
actual native qualification and all existing local regressions; other hosts remain explicitly
unavailable for dedicated placement. No source observation or headless benchmark establishes
native support, and no functionality is disabled merely to obtain a passing measurement.
