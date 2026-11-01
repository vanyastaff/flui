# Multi-owner UI runtime design

- **Status:** Draft for owner review; no production implementation in this change.
- **Contract:** [requirements.md](requirements.md). **Work:** [tasks.md](tasks.md).
- **Decision:** [ADR-0284](../../../adr/ADR-0284-ui-runtimes-may-have-dedicated-owners.md).

## Current producers and consumers

At the specified base, `crates/flui-app/src/app/runner/host.rs` resolves `APP_RUNTIME` for
platform access, storage, fonts, executors and owner wakes. `AppRuntime` in `app/runtime.rs`
holds `InstalledHost`, renderer registrations and shared engine services. `InstalledHost`
combines logical `OwnerHost`, `NativeBindings`, display state and `OwnerEffects`.
`owner_dispatch.rs` uses owner-thread checks and TLS; `RuntimeTask::Frame` carries an
owner-local `FrameBinding` and must not be placed in a remote queue.

`main_window.rs`, `secondary_window.rs` and `window_install.rs` construct roots and publish
native/runtime routing. `desktop.rs`, `android.rs`, `ios.rs` and `web.rs` bind platform frame
callbacks to runtime execution. `raster_lane.rs` pumps the native raster owner inline; Web
uses its direct path. These are actual consumers to migrate, not replaced by a standalone
executor that none of them calls. `flui-runtime` keeps `OwnerHost`, `UiRuntime::pump` and the
owner transaction free of host and GPU dependencies.

## Ownership and terms

The native owner owns the OS loop, windows, native text/accessibility endpoints, surface
callbacks and native raster objects. A UI owner owns one or more local runtimes; a dedicated
owner initially owns exactly the newly launched runtime and all its presentations. The
default UI owner may remain colocated with the native owner. A coordinator belongs to one
application run, not the process, and controls admissions and global exit.

| Owner | State kept there | Allowed crossings |
|---|---|---|
| Native owner | `OwnerPlatform`, `Rc<dyn HostWindow>`, native handlers, renderer/frame bindings, display and native text state | Owned observations, versioned commands, scene envelopes and completion tickets |
| UI owner | `OwnerHost`, `UiRuntime`, views/elements/render objects, signals, futures, local providers and callbacks | Typed input, immutable publication, owned scene output, weak addressed wake/control endpoints |
| Application coordinator | Owner membership, outstanding launch/quit receipts, process service lifetime and exit policy | Bounded control messages; never references to UI state or arbitrary UI closures |

Do not make all of `AppRuntime` shared or clone `InstalledHost` onto a worker. Extract native
and application responsibilities; keep one owner-local logical host per UI owner. TLS remains
an owner-local entry mechanism, not a cross-thread directory. Resolve remote endpoints from
the coordinator's run-scoped registry; a callback captures an addressed weak endpoint rather
than looking up whichever `APP_RUNTIME` happens to exist on its calling thread.

## Launch API and compatibility

Keep `Application::new`, existing root-taking APIs, `open_window`, `WindowPolicy::Isolated`
and `WindowPolicy::Shared` local with their current bounds. A new dedicated entry takes the
factory described in ADR-0284. It is called on the application's native owner, reserves a
slot and starts a UI owner. `AppHandle` remains the closed, transferable application-control
handle; it does not become an arbitrary closure executor.

The factory returns a root and local `RuntimeWindowConfig` on its destination. This new
private-field type exposes only window/runtime options and local capability registration;
it cannot set application-wide policy. Split native window
options into owned data after construction. Do not transfer `AppConfig`, local capability
providers, dev hooks, input taps, error builders or close handlers back to the native thread.
Window-local hooks run on the UI owner. Application-wide options come from the already-running
app and cannot be overridden in the dedicated config. Unsupported
local providers fail launch with a capability-specific reason, not a missing service later.

| Current `AppConfig` field group | Dedicated configuration disposition |
|---|---|
| `application_identity`, `diagnostics_profile`, `exit_policy`, `executors`, `services`, `storage_dir`, `memory_reclaimers` | Application coordinator owns these; absent from the new config. Per-owner resources register with the coordinator's existing reclaim/exit protocol. |
| `title`, `size`, `min_size`, `max_size`, `position`, `mode`, `resizable`, `decorations`, `os_options`, `frame_latency`, `input_latency`, `surface_color`, `placement` | Owned window options sent to native owner; registration/recovery retain presentation identity. |
| `motion_override`, `show_performance_overlay`, `pointer_resampling`, `frame_failure_detail` | Local runtime policy; frame-failure privacy is at least as restrictive as the application policy. |
| `frame_failure_handler`, `input_tap`, `error_view_builder`, `close_request_handler` | Construct and execute on UI owner; never copied into native command envelopes. |
| `dev_reload`, `dev_agent` | Local hook endpoints per UI owner, with the existing application's tool service routed by runtime identity; no duplicate process server or cross-thread local hook. |

Keep existing public `AppConfig` fields and local entry signatures. Extract common internal
option conversion without replacing the old DTO's fields or adding a `Send` bound to it.
The new config has real use at dedicated launch; its builder is not a second application
bootstrap API. Provider factories run there, while native services are acquired through
addressed capabilities.

The single new public factory `Send` bound needs explicit acceptance of its named policy
exception. Audit facade and SDK re-exports and SemVer before implementation; count every
actual rustdoc position introduced by re-export/impl exposure, not a guessed source count.
Do not modify the shrinking allowlists until the exception is accepted. No new unsafe
auto-trait implementation is expected. No `OwnerExecutor` public trait is needed for two
internal delivery variants.

## Addressing and transport

Use a typed generational owner identity, an application-run identity, `UiRuntimeId` and
`PresentationAddress`; reuse existing identity machinery where it already proves these
properties. Per-operation identity adds install ticket, request sequence, text/focus revision,
surface generation or device epoch. Exhaustion is a typed terminal admission failure, never
wraparound. A thread ID only records execution for diagnostics.

The private route has local and remote forms. Both produce the same `RuntimeOperation` or
local `RuntimeTask` only after admission on the destination owner. Remote payloads are a
closed owned vocabulary; `FrameBinding`, `Rc`, arbitrary callbacks and native objects are
excluded. Compiler assertions prove actual payload transferability. Remote `WindowStateChanged`
must carry a versioned snapshot: its current instruction to read a local native window
cannot be forwarded unchanged.

| Lane | Proposed initial bound | Overload and ordering |
|---|---:|---|
| Owner admissions | 8 dedicated owners and 16 outstanding launches per app | Refuse before acceptance; bounds include starting and retiring owners. Make limits explicit run configuration, not an environment-variable escape. |
| Ordinary addressed commands/input | 256 entries and 4 MiB retained owned payload per owner | Return full before accepting app requests. Native delivery has no fallible caller: pause/drain only where the host permits, otherwise terminate the affected input session with explicit reset/cancellation and a surfaced delivery failure; never silently lose key-up/IME commit. |
| Latest scene | One pending scene per presentation, plus one in consumption | Replace only the matching address/generation; release replaced resources outside guards; no control acknowledgements here. |
| Lifecycle/recovery | Reserved state per admitted owner/presentation and bounded receipt slots | Close/retire and recovery obligations survive saturated ordinary traffic; repeated notices coalesce only when their semantics permit it. |
| Completion | One reserved settlement slot per accepted request | Receipt capacity reserved at admission; receiver drop abandons observation, not the operation. |

The byte limit covers shared backing buffers retained by messages, decoded payloads and
temporary copies; maximum single-message size is checked before copying. Scene size and
resource limits must reuse the engine's limits or add explicit checked limits before remote
submission. The numbers above are design defaults to validate under saturation and memory
measurements, not measured optimal settings. Ordering is per admitted owner stream; native
terminal fencing prevents ordinary messages overtaking an accepted close. Lifecycle control
may bypass bulk traffic only without violating previously accepted obligations.

Enqueue commits before wake. A pending-wake obligation is retained across hook absence,
replacement or panic; registration rechecks it. A failed notification must either roll back
an unaccepted request or settle an accepted one through the reliable failure path. Borrowed
guards are gone before wake, waker clone/drop, callbacks, tracing and destruction.

## Native integration without synchronous owner RPC

Win32 initially retains a single native event loop; it does not require a second `Platform::run`
or pretend the current native globals are safe to duplicate. The remote UI constructs scenes;
the native presentation consumes them through its existing raster resources. Surface acquire,
resize, draw and release stay with those resources. Frame opportunities and demand are
addressed messages, and the native loop never waits synchronously for a UI response.

Native text and accessibility requests can be synchronous. Preserve the native endpoint's
authoritative text/session protocol, publish owned state with document/selection/focus
revisions, and serialize edits through tickets acknowledged by the UI owner. Native query
responses may use only a coherent committed publication; no optimistic acknowledgement of
an edit that the UI can subsequently reject. A native synchronous edit that cannot honor
that contract must use the backend's supported deferral mechanism; if none exists, dedicated
admission on that backend remains unavailable. The implementation must resolve this against
the actual TSF and text-store contract before enabling Win32, not hide it with a timeout RPC.

This is a known unresolved integration proof, not an assumption that a snapshot is sufficient.
The actual TSF path in `crates/flui-native/src/text/windows/tsf/mod.rs` holds
`Rc<dyn TextStore>`; `crates/flui-platform/src/text_store/lock.rs` defines owner-local
read/edit closures. ADR-0090 §1 and ADR-0135 §1–§4 remain binding. MO-T5 must first demonstrate
the equivalent edit/lock/commit protocol through that real consumer, including application
edit constraints and composition completion. If it needs a new remote text backend or changes
the single editing authority, submit a narrowly scoped text ADR before implementation; this
spec does not authorize changing the existing pull trait to `Send` or serving stale text.

The alternative to evaluate if this proof fails is a dedicated Win32 native loop colocated
with its UI owner, preserving local TSF. That requires proving independent window-class,
COM/TSF, process-service and loop lifetimes in the actual FLUI host; WPF's success does not
prove those properties here. Record the counterexample and amend ADR-0284's initial native
placement before adopting it. This dependency blocks Win32 availability, not the headless
transport and current-runner migration work.

Mirror inventory for each publication: text, selection/composition, document revision, focus
session, caret/range geometry and scale, semantic identities/properties/actions, window
visibility/geometry/insets/display, surface generation. Test insertion/removal/reorder,
normalization, hidden nodes, focus loss, stale geometry and retirement. Layout-dependent
queries identify the layout revision. Do not share mutable UI memory to avoid this inventory.

Clipboard and native drag sessions remain native-owned; payload admission and asynchronous
answers name session and destination version. A synchronous same-owner document stays local;
cross-owner consumers use a document authority with typed versions, not a framework-wide
shared signal graph. Capability providers must be constructed on the UI owner or reached
through their existing typed cross-thread service, never cloned solely because they are `Rc`.

## Installation, failure and shutdown

| State | Owner of obligation | Transition and rollback |
|---|---|---|
| Reserved | Coordinator | Validate host/configuration/limits; retain factory and receipt; thread-spawn failure refuses and releases reservation. |
| Starting | UI owner plus native install ticket | Factory executes once on UI owner; native window prepared hidden. Mount/renderer/provider failure retires both partial sides before settlement. |
| Mounted | Addressed UI and native routes | Commit both routes before exposing window; receipt settles mounted/wired/redraw-requested. A pending close can prevent reveal. |
| Retiring | Coordinator and each owner | Fence ingress, invalidate tickets, finish/refuse accepted work, retire scene/surface relationships, flush persistence and drop owner-local state. |
| Retired | Coordinator | Both sides acknowledged; thread completion observed, membership released; no stale wake can recreate a runtime. |

Before successful thread spawn a rejected factory's transferable captures may drop on the
caller. After transfer the factory and all resulting local values are disposed on the new
owner, even if launch is cancelled before execution. Each native partial resource is released
on its native owner. User code runs outside all coordinator/transport guards. Catch failures
at the existing owner-turn boundary, preserve the first failure, and aggregate diagnostics
without losing captured payloads or invoking arbitrary formatting under a lock.

Global quit is consultation followed by commit, not sequentially destroying owners while
asking others. Consult every presentation's close guard on its UI owner using a quit-round
ticket. Any refusal cancels that round; holds retain addressed obligations; a new round
invalidates old answers. Fence new installations at commit, not at an arbitrary later join.
Exit policy counts live and starting owners/presentations, including an owner whose primary
window closed. Application persistence/services/pools retire once after runtime obligations.

During retirement the native loop continues serving operations needed to release resources.
Joining happens only after the worker signals completion, never while a callback holds native
state. A watchdog reports a stuck owner but cannot detach it and report success. A permanently
blocked callback or double panic can still require process termination under existing policy;
the protocol does not promise recovery from arbitrary unreturning user code.

## Runner and consumer migration inventory

| Existing consumer | Required migration and removal | Acceptance |
|---|---|---|
| `app/runtime.rs`, `runner/host.rs`, `installed_host.rs`, `native_bindings.rs` | Separate run coordinator, native resources and local logical owner. Remove assumption that native TLS contains every UI runtime; update globals grants with real ownership. | MO1, MO5, MO9, MO13; replacement/teardown tests. |
| `owner_dispatch.rs`, `window_routes.rs`, `window_install.rs`, `main_window.rs`, `secondary_window.rs` | One addressed local/remote admission path; retain local APIs and add explicit factory launch; remove direct native-to-runtime entry for remote routes. | MO2, MO3, MO5, MO6; old local consumer tests plus dedicated production consumer. |
| `desktop.rs`, `frame_driver.rs`, `frame_pacing.rs`, `window_renderer.rs`, `surface_lifecycle.rs`, `raster_lane.rs` | Native frame resource binding stays local; UI pumping moves to selected owner; owned scenes and acknowledgements connect them. Remove remote reliance on local `FrameBinding`. | MO4, MO8; native redraw/readback and generation tests. |
| `device_recovery.rs`, `gpu_loss.rs`, `native_retirement.rs`, `terminal.rs` | Address recovery and retirement to correct owner; settle all receipts; no exit before acknowledgements. | MO8–MO10; failure competition and next-operation tests. |
| `fonts.rs`, `display_host.rs`, `native_text_sizing.rs`, capability registration | Fan out versioned publications through owner routes; share process services once; rebuild local provider state on its owner. | MO7, MO13; font/preference/geometry replacement tests. |
| `application_control.rs`, `Application`, facade and SDK exports | Keep closed `AppHandle` ingress; coordinate global quit and retry across owners; preserve existing bounds; expose only accepted dedicated API. | MO2, MO9; SemVer and facade closure gates. |
| Win32 native callbacks/text/accessibility | Convert native observations to addressed owned ingress; coherent synchronous native publication and action/edit tickets; keep OS panic boundaries. | Entire native matrix, including actual TSF, UIA and screen-reader runs. |
| Native Wayland, native X11 and winit fallback | Migrate shared route without enabling dedicated launch; evaluate connection/event-loop ownership and native query contracts separately. | Shared-path regression; explicit unavailable result until each real host suite passes. |
| `ios.rs`, `session_controller.rs`, `android.rs`, `web.rs`, AppKit desktop path | Preserve scene/activity/DOM and main-thread lifecycle. Use common route in local mode; dedicated unavailable. Remove duplicate lifecycle logic, not platform semantics. | Existing lifecycle/owner tests and target checks; physical/simulator/browser evidence labeled separately. |
| `flui-testing`, native testing kits, driver/devtools, examples and packages | Add reusable owner controls and native scenario to existing kits; trace owner/address identity; migrate local runner fixtures; demonstrate opt-in and typed refusal. | MO11, MO12, MO14; no bespoke substitute runtime or dead public API. |

No per-window GPU-context rewrite or removal of the valid Web direct raster path is bundled
into this migration. Remove obsolete ownership/dispatch bypasses only after their real
consumers use the shared contract. Current platform behavior must remain available while
dedicated admission is closed. Every existing runner is accounted for even when its native
execution cannot be performed on the development host.

## Verification and status

The requirement matrix and tasks distinguish proposed tests, executed source checks,
historical headless evidence and missing native evidence. Integrations live under existing
`tests/main.rs` modules; native pure/model tests stay beside their modules and native
recordings/conformance stay in the native test suite. Test controls belong in `testing`
kits (ADR-0243). Run each meaningful new behavioral test with the implementation removed
or routing forced local to demonstrate the intended failure, without touching other worktrees.

Default behavior, overload, reentry and teardown are release blockers, not follow-ups to a
performance demo. Support documentation remains unchanged except an explicit unavailable
note until the native matrix is complete. Remaining host limitations stay visible even if
the Win32 capability later qualifies. Accepting the design is distinct from enabling it.
