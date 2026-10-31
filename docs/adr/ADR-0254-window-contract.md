# ADR-0254: One window truth: a state snapshot, admitted commands, one handler and an owner-local window

- **Status:** Accepted (2026-10-20). Decisions 1 to 5 are implemented: the snapshot, admitted
  commands, the per-frame verbs and the material command, the window handler and the
  `AppHandler` passed to `Platform::run`, and the owner-local window with its surface target and
  redraw requester. What remains of decisions 6 to 8 lands as separate changes, in the order of
  the platform-layer feature's tasks. A platform's row in `docs/platform-parity.md` moves only
  after a run on that platform (Confirmation): the AppKit and UIKit hosts of decision 5 are
  type-checked here and run on a Mac separately.
- **Date:** 2026-10-20
- **Superseded-by:** [ADR-0276](ADR-0276-a-window-publishes-its-insets-and-the-keyboards-motion.md)
  in part: the snapshot field `safe_area` is `insets: WindowInsets`.
- **Amended:** 2026-10-27 — decision 4, approved by the owner: (1) a `WindowHandler` method takes
  no `&OwnerPlatform`, because `WindowHandler` is the contract's and `OwnerPlatform` is the host
  crate's, which the contract cannot name without the edge ADR-0083 and the layers forbid; a
  handler that drives the platform captures what it needs; (2) `frame_requested` answers a
  `FrameOutcome { Produced, Declined }`, because without it the host cannot tell a frame that was
  served from one nothing ran, and a wake deadline delivered to a handler with nothing to draw
  with re-arms on every paint and spins the loop.
- **Amended:** 2026-10-27 — decision 6, following the owner's decision of 2026-10-27 that adds
  `NotYetOnThisHost` to the reasons (ADR-0152): a command's capability carries where it is
  missing, a `CapabilityAbsence` (`NotOnThisPlatform`, `NotOnThisDevice`, `NotYetOnThisHost`),
  set with `WindowCapabilities::unavailable(command, absence)`, and admission refuses the command
  with that reason. The type admits no retryable reason, since a capability stays `Unavailable`
  until the host publishes it again; an absence counts for equality only while its capability is
  unavailable. The verbs outside the command queue (the frame-rate hint, the input regions) answer
  their reason themselves: nothing admits them against the capabilities.
- **Amended:** 2026-10-27 — decision 4, approved by the owner: `AppHandler::exit_policy` answers
  an `ExitAnswer { End, KeepRunning, HostDecides }` instead of a `bool`. An application with no
  exit policy of its own had to answer `false`, which silently changed the hosts' behaviour (the
  Win32, AppKit and winit loops no longer ended with their last window); `HostDecides` keeps the
  host's own default, as with no handler bound.
- **Amended:** 2026-10-27 — decision 5, approved by the owner: (1) a thread that is not a window's
  owner asks it for a frame through `PlatformWindow::redraw_requester()`, a `Send + Sync`
  `RedrawRequester` (any `Fn() + Send + Sync` closure is one), not through a `PlatformProxy`
  verb, because a verb naming a window would need a lookup from the window's identity to the
  window in every transport, and `flui-runtime`, which asks for frames from signals written on
  workers, has no edge to the host crate (ADR-0083); (2) a deferred open resolves to an
  `OpenedWindow`, the window's identity on any thread and the window on its owner thread only;
  (3) an AppKit window asked for a frame from another thread queues the redraw on its owner lane
  and returns at once, where it used to block until the lane ran it.
- **Supersedes:**
  - [ADR-0237](ADR-0237-window-state-and-host-notices.md), in part: the window-scoped getters and
    per-fact callbacks that sit beside the snapshot; decision 3's refusal inside `apply` (it moves
    to `WindowCommand::admit`, the rule "refused exactly when the capability is false" stays); the
    registrations that deliver decisions 7 and 8's notices (`Platform::on_host_notice`,
    `Platform::on_session_end`), which become `AppHandler` methods. The snapshot with its
    revision, the command and capability triad, the payload-free state notification, owner-turn
    application and `WindowHandle` (decision 4), placement and the notices themselves stay.
  - [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md), in part: the window itself as the
    `Send + Sync` surface target (decision 1's `Arc<dyn PlatformWindow>` handed to
    `Renderer::new`; decision 5's `Unavailable` obligation on every `PlatformWindow`, which moves
    to the separate surface target the window hands out) and decision 6's
    `on_surface_status_change` registration (the handler's `surface_changed` obligation; the
    release-and-rebuild semantics stay).
  - [ADR-0213](ADR-0213-input-capabilities.md), in part: the delivery rules of decision 3 that
    exist because `on_input` is registered after `open_window` returns (the retained snapshot,
    bounded retries per registration, the retirement of a replaced callback), and decision 6. The
    snapshot-first ordering and the reentrancy and panic-containment rules stay.
  - [ADR-0082](ADR-0082-platform-api-contract-crate.md) §4 (registered callbacks lose `Send`, both
    steps: the registrations they migrate are replaced by the handler) and the raw-handle impls on
    `dyn PlatformWindow` and `dyn HostWindow` in §3's second change.
  - [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §3, in part: the sentence that the
    owner lane never carries closures. The deferred window open carries one `Send` factory
    (decision 4). §1 and §6, in part: the one-shot `PlatformReadyCallback` (`on_ready`) `run`
    took, which is `AppHandler::ready` (decision 4); the owner capability it hands over and its
    loop scope stay.
  - [ADR-0044](ADR-0044-driver-loop-hybrid.md) §7, in part: the registered
    `Platform::set_wake_deadline_hook`, which is `AppHandler::wake_deadline` (decision 4); the
    wall-clock wake the loop computes from its answer stays.
  - [ADR-0093](ADR-0093-router-is-the-primary-navigation-api.md), in part: the
    `Platform::on_open_urls` registration it names as a deep link's source, deleted by decision
    4; the opened URLs and files reach the handler with the activation feature.
  - [ADR-0222](ADR-0222-window-appearance-is-a-presentation-observation.md), the delivery shape
    only: the appearance is a field of the window state snapshot and its change arrives through
    the handler's state-changed obligation, not through `appearance()` and
    `on_appearance_changed`; the observation rule stays.
  - [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) decision 5, the delivery shape
    only: the window's display is a field of the snapshot and its change arrives the same way;
    the snapshot and the refresh period stay.
- **Related:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) (execution
  state), [ADR-0039](ADR-0039-event-loop-affinity-capability.md) (the owner proof and the owner
  lane), [ADR-0045](ADR-0045-raster-lane.md) (the raster thread),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (escape modules are Evolving),
  [ADR-0123](ADR-0123-exceptional-presentation-close.md) (the close path),
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) (crate names and the closed list),
  [ADR-0152](ADR-0152-capability-seam-revised.md) (`Unsupported`, shared with this contract),
  [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) (deferred dispatch),
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (backend conformance),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (version gates).
- **Sources:** other projects are read at pinned commits and cited as `owner/repo@sha path:line`:
  winit 0.31.0-beta.3 (`rust-windowing/winit@1893565716`), GPUI (`zed-industries/zed@fae8a35482`),
  Slint (`slint-ui/slint@277e7676ce`), Masonry (`linebender/xilem@271a27a6d4`), SDL3
  (`libsdl-org/SDL@d0183f0ee3`), Flutter's embedder (`flutter/flutter@6597889f57`),
  raw-window-handle 0.6.2 (`rust-windowing/raw-window-handle@1b859486d7`) and wgpu
  (`gfx-rs/wgpu@3050396df2`). Counts and lines of FLUI's own tree are at `main` `083b0f75d`.

Crate names (ADR-0151). The contract crate is `flui-platform` and the host crate is
`flui-native`. This record says
"contract crate" and "host crate".

## Context and problem statement

ADR-0237 gave a window a revisioned `WindowState`, a `WindowCommand` set and the
`WindowCapabilities` that say which commands the window honours. The triad was compared with
winit, GPUI, Slint, Masonry, SDL3, Flutter's embedder and raw-window-handle: none of them has a
snapshot with commands checked against capabilities, and none makes "unsupported" a typed,
queryable answer. winit's setters are silent no-ops with a doc line per platform
([rust-windowing/winit@1893565716 winit-core/src/window.rs:874-877,1155-1185](https://github.com/rust-windowing/winit/blob/1893565716/winit-core/src/window.rs#L874-L877)); GPUI's default to
no-ops ([zed-industries/zed@fae8a35482 crates/gpui/src/platform.rs:1332-1342](https://github.com/zed-industries/zed/blob/fae8a35482/crates/gpui/src/platform.rs#L1332-L1342)); Slint hands the
host desired properties and warns that an incomplete two-way protocol misbehaves
([slint-ui/slint@277e7676ce internal/core/window.rs:60-76](https://github.com/slint-ui/slint/blob/277e7676ce/internal/core/window.rs#L60-L76)); Masonry's signals return nothing and
include a non-idempotent `ToggleMaximized` ([linebender/xilem@271a27a6d4
masonry_core/src/app/render_root.rs:228-306](https://github.com/linebender/xilem/blob/271a27a6d4/masonry_core/src/app/render_root.rs#L228-L306)); SDL3 returns a `bool` and a thread-local error
string ([libsdl-org/SDL@d0183f0ee3 include/SDL3/SDL_video.h](https://github.com/libsdl-org/SDL/blob/d0183f0ee3/include/SDL3/SDL_video.h), `SDL_GetError` throughout) and
makes size and mode changes requests that `SDL_SyncWindow` waits for (`:1041-1046`); Flutter's
embedder has no window commands and takes a metrics snapshot instead
([flutter/flutter@6597889f57 engine/src/flutter/shell/platform/embedder/embedder.h:1098-1148](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/shell/platform/embedder/embedder.h#L1098-L1148)).
The triad is the best shape of the group. `PlatformWindow` around it is not finished:

- **Three sources per fact.** `is_focused`, `is_visible`, `physical_size`, `logical_size`,
  `scale_factor`, `metrics`, `appearance`, `display_id`, `safe_area_insets`, `execution_state`,
  `is_active` and `is_hovered` are getters beside `WindowState`, and most have an `on_*` callback
  of their own. ADR-0237's context names the failure: "a state assembled from six events that
  drift from readback".
- **Twins of commands.** `show` and `WindowShowError`, `activate` and `close` repeat `Show`,
  `Activate` and the close path, with a second spelling of "unsupported" and, for `close`, a
  35-line doc listing five host-defined behaviours.
- **Admission by convention.** `apply` takes a bare `WindowCommand`; "an implementation begins with
  `admit`" is a sentence in a doc comment and a conformance row.
- **Fourteen registrations.** 21 of the trait's 39 defaulted methods do nothing (`let _ =
  callback;`): all 14 `on_*`, three setters and four empty bodies. That is the recurring defect
  "accepted work dropped when a hook is missing". The registrations are made after `open_window`
  returns, so ADR-0213 decision 3 spends a paragraph on a retained snapshot, a queue of early
  input, bounded retries and the retirement of a replaced callback. 13 of the 14 are `+ Send`, which
  is why `flui-app` keeps its `!Send` UI runtime in a `thread_local!`
  (`crates/flui-app/src/app/runner/host.rs:431-441`).
- **A `Send + Sync` window for one consumer.** The raster thread needs a surface target (ADR-0063
  decision 1), which needs `HasWindowHandle + HasDisplayHandle + Send + Sync`
  ([gfx-rs/wgpu@3050396df2 wgpu/src/api/surface.rs:266](https://github.com/gfx-rs/wgpu/blob/3050396df2/wgpu/src/api/surface.rs#L266)), not 51 methods.
  The price is a wrong-thread `request_redraw` that compiles and is unsound on macOS, and about
  50 lines of doc stating the rule instead of a type stating it
  (`crates/flui-platform/src/platform_window.rs:247-298`).
- **Nine spellings of "not here".** Nine enums each define their own `Unsupported` or
  `Unavailable` (ADR-0151 lists them with their lines). The per-frame verbs return `()`: the Win32
  host logs a failed backdrop call, so an app cannot know Mica was ignored
  (`crates/flui-native/src/window/windows/backdrop.rs:491-505`).
  `WindowBackgroundAppearance` carries two Windows-only variants in a Stable contract and has no
  caller.
- **An escape hatch nobody can reach.** `as_any` is a required method of the open window trait
  (`platform_window.rs:899`); the per-OS extension traits sit in the host crate, which the facade
  does not publish (`crates/flui-native/src/window/windows/backdrop.rs:48`,
  `crates/flui-native/src/window/apple/macos/ext.rs:41`; `src/lib.rs:147`).
  `WindowEvent` and `Platform::on_window_event` have no production caller above the host.

winit pairs a `Send + Sync` window with a single handler trait and pays with blocking
cross-thread marshalling ([rust-windowing/winit@1893565716 winit-core/src/window.rs:620-631](https://github.com/rust-windowing/winit/blob/1893565716/winit-core/src/window.rs#L620-L631),
[winit-core/src/application/mod.rs:10](https://github.com/rust-windowing/winit/blob/1893565716/winit-core/src/application/mod.rs#L10)). GPUI and Slint keep the window `!Send`. Slint also keeps a
richer hidden tier for its own hosts, and a third-party host is second class there
([slint-ui/slint@277e7676ce internal/core/window.rs:225-341](https://github.com/slint-ui/slint/blob/277e7676ce/internal/core/window.rs#L225-L341), [internal/core/platform.rs:564-575](https://github.com/slint-ui/slint/blob/277e7676ce/internal/core/platform.rs#L564-L575)).

The question this record answers: what shape should a window and its host have so that each window
fact has one source, a command is admitted by type, user code is called through one handler, and
the thread rule is checked by the compiler instead of documented?

## Decision drivers

- Each window fact has one source: three sources per fact drift, and ADR-0237 named the failure, "a
  state assembled from six events that drift from readback".
- Admission is a type, not a sentence in a doc comment and a conformance row.
- Accepted work is never dropped because a hook is missing: one required method per obligation and
  no inert default bodies (a recurring defect, AGENTS.md).
- The handler exists before the first event, so no registration gap, retained snapshot or
  early-input queue is needed (ADR-0213).
- The thread rule is checked by the compiler: the raster thread needs only a surface target
  (`HasWindowHandle + HasDisplayHandle + Send + Sync`), a wrong-thread `request_redraw` must not
  compile, and `flui-app`'s `thread_local!` for the `!Send` UI runtime exists only because the
  callbacks are `Send`.
- User code runs on the owner thread, at the owner's next turn and outside any lock or borrow of
  the host; a native call that re-enters the window procedure stays out of user code, and a
  panicking handler method is contained so the next obligation is still delivered.
- "Not here" has one vocabulary shared with the capability seam (ADR-0152), and the contract never
  carries an OS version (ADR-0244).
- No second, hidden tier: a third-party host gets the vocabulary a first-party one gets.
- Sealing is reversible and opening is not; an open trait has no inert defaults, and every facet
  beyond its required core is a separate trait behind an `Option`.
- The escape hatch is reachable, typed and total: it never panics on a window of a foreign host.
- A command is checked against the capabilities the window has when it is applied, so the race
  between admission and application is a typed error and not an OS failure.
- CI executes only Linux and wasm32 (AGENTS.md), so each native host shows a run, and a
  platform's row is ✅ only after one.

## Considered options

1. The window contract decided below: a state snapshot as the only source of facts, admitted
   commands, one handler passed at open, and an owner-local window with a separate surface target.
2. Keep the getters as conveniences beside the snapshot.
3. State delivered as events with payloads (winit).
4. A desired state the host converges to (Slint).
5. Per-capability typed handles (`window.maximizer() -> Option<..>`).
6. Admission by convention plus a conformance row (today).
7. An `ApplyError` without `Superseded`.
8. Per-frame verbs and the backdrop through the command queue.
9. Keep `Send + Sync` and add owner-proof registrars (ADR-0082 §4 as written).
10. winit's cross-thread marshalling.
11. One event enum into one closure (Slint, Masonry).
12. A pull queue (SDL).
13. Keep the closures and make every `on_*` return `Result<(), Unsupported>`.
14. A richer hidden tier for first-party hosts (Slint's `InternalToken`).
15. An open `Platform` and `HostWindow`.
16. Per-OS fields in `WindowOptions` (GPUI) or untyped property keys (SDL).
17. winit's extension traits (downcast and `unwrap`).
18. An OS-version query in the contract.

## Decision

Chosen option: 1, because it gives each window fact one source, makes admission a type, replaces
the fourteen optional registrations with one handler that has no inert defaults, and moves the
thread rule from documentation into the compiler.

Names below are the contract's, `WindowHandler` included (its obligations name contract types
only, and `flui-runtime` implements it); `AppHandler`, which names `OwnerPlatform`, is a trait of
the host crate (ADR-0151 section 9).

### 1. `WindowState` is the only source of window facts

`WindowState` stays the revisioned immutable snapshot of ADR-0237 decision 2: it describes what the
OS reports, an equal sample keeps the revision, an exhausted revision refuses. Three changes:

- **`WindowCapabilities` is a field of the snapshot.** A change in what a window honours (a
  compositor revising what it allows) is a state change: a new revision and one notification.
- **The window-scoped getters and their callbacks are deleted:** `is_focused`, `is_visible`,
  `physical_size`, `logical_size`, `scale_factor`, `metrics`, `appearance`, `display_id`,
  `safe_area_insets`, `execution_state`, `is_active`, `is_hovered`, `capabilities`, and
  `on_resize`, `on_active_status_change`, `on_visibility_status_change`, `on_hover_status_change`,
  `on_appearance_changed`, `on_display_changed`, `on_safe_area_change`,
  `on_execution_state_change`. Their facts are fields of the snapshot: those it has (focus,
  visibility, display) and those it gains (scale factor and device-pixel metrics, appearance,
  safe-area insets, execution state, hover).
- **Each fact keeps the rule its record set.** The light/dark theme is the user's as applied to the
  window, never the chrome's own (ADR-0222); the display is named by the display snapshot's
  identity (ADR-0236); execution state is independent of focus, visibility and the GPU surface
  (ADR-0035); a report that is not a drawable area (a zero size, a scale that is not finite and
  positive) is never published. Only where the value is read and how its change is announced
  move.

A consumer that needs to know which fact changed compares the new snapshot with the last one it
saw; revision equality is the cheap test ADR-0237 already provides. Input-derived values are not
window state: `mouse_position` and `modifiers` are deleted from the window, and pointer position
and modifiers come from the input stream.

### 2. Commands are admitted, and have no method twins

`show`, `activate` and `close` are deleted as methods, together with `WindowShowError`. Every
change to a window goes through a command, closing included: `WindowCommand` gains a `Close`
variant beside `Show` and `Activate`, so one timing rule holds: an accepted command is a request,
and its outcome is read from the next snapshot and, for a close, from the handler's `closed`. The vetoed close path of ADR-0237 and ADR-0123 is kept as a handler
obligation (decision 4): the veto lives in `close_requested`.

`reveal_after_first_frame` is not carried over as a method either. The reveal policy is data in the
window's options, `WindowOptions`' `WindowReveal`, and the host honours the policy the window
was opened with; there is nothing for the embedder to call.

Admission is a type:

- `WindowCommand::admit(&WindowCapabilities) -> Result<AdmittedCommand, AdmissionError>` is the
  only constructor of `AdmittedCommand`. It refuses invalid geometry before any OS call and
  refuses a command whose capability is `Unavailable`. The token carries the capabilities revision
  it was admitted against: the revision of the capabilities in the snapshot, which moves when what
  the window honours changes and not with every state change.
- `PlatformWindow::apply(AdmittedCommand) -> Result<(), ApplyError>`. A host can no longer
  receive an unvalidated command, and its `apply` has no admission arm to remember.
- The errors are split by who can cause them. `AdmissionError` is `Unsupported(Unsupported)` or
  `InvalidGeometry`, the caller's. `ApplyError` is `Closed`, `Superseded` or `Platform`, the
  world's. `Superseded` means the capabilities revision moved between admission and application
  (commands apply at the owner's next turn, so a change can land in between): the command was
  checked against capabilities the window no longer has, and the caller admits it again. Without
  that arm the race "admitted at revision N, applied at N+1" would reach `Platform` and could not be
  told from a failure of the OS.
- The framework's caller of `admit` is the queue behind `WindowControl::submit` (ADR-0237
  decision 4): it admits at submit time against the snapshot's capabilities, then applies the
  `AdmittedCommand` on the owner's next turn, in submission order, as before. `submit` returns
  `Result<(), SubmitError>`, where `SubmitError` is `Admission(AdmissionError)`, the caller's
  refusal at submit time, or `Closed`, the window is gone and there is no queue to take the
  command.

**The limit, stated in the docs of `AdmittedCommand`:** the token proves that the command was
validated against the capabilities read at admission, and `apply` compares the token's revision
with the window's current one, so a command is not applied under capabilities it was not checked
against. The OS still keeps the last word, and it speaks through `ApplyError::Platform` and the next
snapshot. The remaining test obligation moves from "`apply` refuses exactly when a capability is
false" to "a window's capabilities are honest".

### 3. Verbs the window cannot always honour answer in the same type

- `set_frame_rate_hint` and `set_input_regions` return `Result<(), Unsupported>` and have an
  `Availability` in the capabilities (the frame-rate hint in `WindowCapabilities`; input regions in
  `InputCapabilities::input_regions`, which already answers it, ADR-0213). They stay verbs and do
  not enter the command queue: commands apply at the owner's next turn, and these are sent every
  frame.
- The backdrop becomes one cross-platform command with a capability, replacing
  `WindowBackgroundAppearance` and its Windows-only variants. A window system that cannot give the
  requested material answers `Unsupported` instead of logging, and the OS-version gate behind it
  stays a `Since<F>` in the host (decision 6). OS-specific materials stay reachable through
  decision 8.
- A refusal reaches the caller as `Unsupported`: through `AdmissionError::Unsupported` when the
  change is a command, directly for the verbs.

### 4. One handler is passed at open

`open_window(options, handler)` takes an `Rc<dyn WindowHandler>`. Its methods are called on the
owner thread, the one that holds the handler's `Rc`. They take no platform argument: the owner's
platform, `OwnerPlatform`, is a host type the contract cannot name (ADR-0083), so a handler that
drives the platform captures what it needs. It has one method per obligation, not per fact, and
none has a default body:

| Method | Called when | Returns |
|---|---|---|
| `input` | A platform input event for this window arrives. The first is the capability snapshot, `PlatformInput::Capabilities` | `DispatchEventResult`, with its deferred forms for an event queued behind a reentrant callback (ADR-0207) |
| `state_changed` | A new snapshot has been published, outside any lock or borrow of the host | Nothing; no payload, the handler re-reads the snapshot |
| `frame_requested` | The platform wants a frame for this window | A `FrameOutcome`: `Produced` when a frame ran, `Declined` when the handler had nothing to run one with; only a produced frame counts as serving work due at one instant (a wake deadline) |
| `close_requested` | The user, the OS or `WindowControl::request_close` asks the window to close | A `CloseDecision`: proceed, or veto and keep the window |
| `closed` | The window has closed, once, after it left the host's tracking | Nothing |
| `surface_changed` | The surface behind the window is about to become invalid, or a valid one must exist again; the two directions keep the asymmetry of ADR-0063 decision 6 | Nothing |

This replaces the 14 `on_*` registrations. What goes with them:

- **The inert defaults.** A required method cannot silently drop what the framework handed it. A
  handler that does not care about an obligation says so in code.
- **The registration gap.** The handler exists before the first event, so the retained snapshot and
  early-input queue of ADR-0213 decision 3, and the retirement of a replaced callback, have no
  case to serve. What stays is ordering: the capability snapshot is the first `input`, later
  changes are ordered with the input around them, and the rules for reentrant delivery and for a
  panicking method (the queued tail keeps its order) hold because the handler can still be
  re-entered. Containment follows the host's existing policy (ADR-0207 §7): the panic is caught at
  the call point in `owner/`, the queued tail is delivered, and the first payload is resurfaced
  afterwards; a second panic during that delivery does not replace the first, and no obligation
  that was accepted before the panic is dropped.
- **The `Send` bound and N slots.** A window has one handler to drop when it closes.

**The cross-thread deferred-open lane** (`PlatformProxy::open_window`) cannot carry an `Rc`. It
carries a `Send` factory, a boxed `FnOnce`, that builds the `Rc<dyn WindowHandler>` on the owner
thread. What the requesting thread receives from a `PendingWindow` is the window's identity, never
the window (the same split ADR-0237 decision 4 makes for `WindowHandle`).

**The same shape holds at application level.** One `AppHandler` passed to `Platform::run`
replaces the registrations `Platform` has today for application-level events and queries
(`on_quit`, `on_reopen`, `on_session_end`, `on_host_notice`, `set_exit_policy_hook`,
`set_wake_deadline_hook`) and the one-shot `PlatformReadyCallback`: one required method per
obligation (`ready` with the `OwnerPlatform`, quit requested, reopened, session end with its
answer, host notice, and the two queries `exit_policy(&OwnerPlatform) -> ExitAnswer` and
`wake_deadline(&OwnerPlatform) -> Option<Instant>`), each receiving `&OwnerPlatform` where the
platform exists. `ExitAnswer` is `End`, `KeepRunning` or `HostDecides`; `HostDecides` leaves the
exit to the host's own default, as with no handler bound (the Win32, AppKit and winit hosts end
the loop, the headless host keeps it running). `AppHandler` is a trait of the host crate, not of the
contract: it names `OwnerPlatform`, so only code that runs a host itself implements it — `flui-app`,
and an example or a test that drives `flui-native` directly (ADR-0151 §9). The other
registrations are not carried over.
`on_window_event` is deleted below. `on_keyboard_layout_change` becomes `pub(crate)` in the host
crate with `keyboard_layout`, which nothing above the host reads. `on_open_urls` is deleted now,
because nothing produces it, and the delivery of opened URLs and files, `opened(urls, files)`,
joins the handler with the activation feature. System preferences are not a registration:
ADR-0172's delivery stands. The notices, the session-end hold and their fan-out are ADR-0237's and
do not change.

`WindowEvent` and `Platform::on_window_event` are deleted: no production caller exists above the
host, and their variants duplicate the snapshot and the handler.

### 5. The window is owner-local; the raster thread gets a surface target

`PlatformWindow` is `!Send + !Sync`: it drops its `Send` and `Sync` supertraits, so no `dyn
PlatformWindow`, and no `Rc` or `Arc` of one, crosses a thread, and a wrong-thread call to a window
method stops compiling instead of being documented. The raster thread receives a separate object:

- a `SurfaceTarget`, `Send + Sync`, implementing `HasWindowHandle + HasDisplayHandle`. The runner
  hands it to `Renderer::new`, which keeps ADR-0063's `WindowTarget` bound, in place of the window;
- it answers rwh's `HandleError::Unavailable` once the native window is destroyed or suspended.
  That obligation, with the per-platform rules of ADR-0063 decision 5 (Win32 identity probe,
  AppKit closed flag, Android only for a terminated window, headless always), moves from the
  window to the target;
- it has `pre_present_notify` as a required method, because the raster thread calls it once per
  presented frame, immediately before the present (an inert default of `PlatformWindow` today,
  `crates/flui-platform/src/platform_window.rs:311`; on Wayland it arms the surface's frame
  callback, which paces `request_redraw` by the compositor). A host without the concept
  returns at once, and its documentation says so; the absence is stated by the host and not
  inherited from a default;
- it carries the only `unsafe impl Send` a host needs, with a `SAFETY` comment that names the
  invariant (what the target holds and how liveness is checked), not the operation.

### 6. One vocabulary for "not here"

`Unsupported { reason }`, defined once in the contract crate by ADR-0152 (decision 2, item 7), is
the vocabulary of the window side too. It replaces the unit `Unsupported` and `Unavailable`
variants of `WindowCommandError`, `WindowShowError`, `CursorError` and the other window-side enums,
which embed it. The reasons are ADR-0152's: `NotOnThisPlatform`, `NotOnThisDevice` (the OS build
or the hardware of this device lacks it), `NotYetOnThisHost` (the OS has it and this host does not
implement it yet; amended 2026-10-27), `NotDeclared`, `Unavailable` (temporary: trying again
may succeed), `NotRegistered` and `NoWindow`. A window builds the value with
`Unsupported::new(reason)`; `Unsupported::of::<C>(reason)`, which names a capability, is for the
seam.

- `Availability` (`Native`, `Emulated`, `Unavailable`; ADR-0213) extends from `InputCapabilities`
  to `WindowCapabilities`. A capability that can be honoured approximately says `Emulated`, which
  a boolean cannot. A command is refused exactly when its capability is `Unavailable`.
- **OS versions never appear in the contract.** `NotOnThisDevice` names no version, and no
  accessor orders the running OS. The gate that produced it stays a `Since<F>` proof in the
  host (ADR-0244), so `if sdk >= 29` cannot be written outside FLUI.
- The surface target's answer stays rwh's `HandleError::Unavailable`: it is that crate's contract,
  not an FLUI `Unsupported`.

### 7. Sealing

- `Platform` and `HostWindow` are sealed with a private supertrait, as Slint seals its `Renderer`
  ([slint-ui/slint@277e7676ce internal/core/renderer.rs:43-52](https://github.com/slint-ui/slint/blob/277e7676ce/internal/core/renderer.rs#L43-L52)).
  An outside `impl` is a compile error that says what the types already enforce
  (`OwnerPlatform::new` is `pub(crate)`, so an outside `Platform` could not run). Sealing is
  reversible; opening is not.
- `PlatformWindow` stays open, for test doubles and the doctest that builds a window from the
  contract crate alone. Two rules keep it open safely. It has no inert defaults. Every facet beyond
  the required core is a separate trait reached through an `Option` handle (as `text_input`,
  `haptics`, `system_chrome` and `drag_source` already are), so adding a facet never breaks an
  implementor.
- **No second, hidden tier.** `PlatformInput` carries pointer, key, IME and drag to every host
  alike. First-party hosts get no richer vocabulary than the one a third party sees.

### 8. The escape hatch is published, typed and total

- `as_any` moves from `PlatformWindow` to `HostWindow`, so the concrete type of a window is no
  longer exposed to every implementor of the open trait.
- Per-OS extension modules are published by the facade as `flui::native::<os>`, behind `cfg` and a
  `native` feature, as an Evolving surface (ADR-0089 decision 5). Their methods take and return
  FLUI types only, answer `Option` or `Unsupported`, and never panic on a window of a foreign
  host (winit `unwrap`s its downcast: [rust-windowing/winit@1893565716
  winit-win32/src/lib.rs:355-358](https://github.com/rust-windowing/winit/blob/1893565716/winit-win32/src/lib.rs#L355-L358)). Raw window and display handles leave only through the
  `raw-window-handle` traits. OS types that are public in the host crate's paths become
  `pub(crate)`.
- Creation-time options that exist for one OS are a typed value for that OS inside
  `WindowOptions`, not per-OS fields in the common struct and not untyped keys.

### 9. What stays, because it is better than the references

- **The triad and its revision rule.** winit's setters are silent no-ops with a doc line; GPUI and
  Slint default to no-ops; Masonry's signals have no result and include a non-idempotent toggle;
  SDL returns a string (the lines are in Context above). The payload-free notification stays too.
- **Owner-turn application of commands** (ADR-0237 decision 4): a native call that re-enters the
  window procedure synchronously stays out of user code.
- **`OwnerPlatform` as a `!Send` proof, `SharedPlatform` as the fence.** winit's window is `Send +
  Sync`; GPUI and Slint get the effect by leaving `Send` off but have no proof type for the loop
  thread.
- **`DispatchEventResult` and its deferred forms.** The references return a plain answer: Masonry a
  synchronous `Handled`, Slint a two-value `Accepted` or `Rejected` ([linebender/xilem@271a27a6d4
  masonry_core/src/app/render_root.rs:548](https://github.com/linebender/xilem/blob/271a27a6d4/masonry_core/src/app/render_root.rs#L548); [slint-ui/slint@277e7676ce
  internal/core/platform.rs:324-333](https://github.com/slint-ui/slint/blob/277e7676ce/internal/core/platform.rs#L324-L333)).
- **`Since<F>` gates inside the host** (ADR-0244). winit has no version mechanism and GPUI
  writes "iOS 26+" in a comment and returns zero where it cannot say ([zed-industries/zed@fae8a35482
  crates/gpui/src/platform.rs:1169-1173](https://github.com/zed-industries/zed/blob/fae8a35482/crates/gpui/src/platform.rs#L1169-L1173)).
- **`HasWindowHandle` and `HasDisplayHandle` as the only upstream traits in the contract**, and the
  `Unavailable` obligation (ADR-0089).

### Consequences

- Bad, because every host window changes: it publishes a larger snapshot, takes `apply` with an
  `AdmittedCommand`, calls one handler, and hands out a surface target; `Platform` and `HostWindow`
  change with them. CI executes only Linux and wasm32 (AGENTS.md), so each native host's change
  shows a run on its platform, and `docs/platform-parity.md` flips a cell only after one. Win32's
  live-resize path must still reach `state_changed` from inside the OS callback.
- Bad, because the `flui-app` runners change: the 14 registrations, repeated across seven runner
  files (some kinds at five to seven sites), become one handler per runner.
- Good, because the `thread_local!` that holds the `!Send` UI runtime loses its stated reason, the
  `Send` callback surface.
- Bad, because 15 implementors of `PlatformWindow` outside the hosts (13 files, in
  `flui-interaction`, `flui-runtime`, `flui-app` and `flui-testing`; counted with
  `rg '^\s*(unsafe )?impl[^/]*PlatformWindow for'`) implement the new shape.
- Good, because those test doubles also shed the `Mutex` and `Arc` that `Send + Sync` forced on
  them.
- Bad, because `Renderer::new` call sites pass the surface target; the conformance rows for the
  `Unavailable` obligation move to it, and `pre_present_notify` moves with them.
- Bad, because `PlatformProxy::open_window` carries a `Send` handler factory, a boxed `FnOnce`, the
  one closure the lane carries; ADR-0039 §3 described the lane as carrying no closures. The factory
  is run once, on the owner, to build the handler; every other request is unchanged.
  `PendingWindow::wait`, `try_take` and the future resolve to the window's identity instead of an
  `Arc<dyn HostWindow>`.
- Bad, because `WindowEvent`, `WindowBackgroundAppearance`, `WindowShowError`,
  `WindowCommandError`, `Platform::on_window_event`, `reveal_after_first_frame` and the deleted
  getters leave the public surface.
- Neutral, because the crates are unpublished, so no external consumer is affected.
- Neutral, because the code lands in separate changes after this record; their order is the
  feature's `tasks.md`.
- Good, because there is one truth per window fact, and one place (the snapshot) to add the next
  one.
- Good, because the `request_redraw` essay and its "violated on two paths" section, and the
  owner-slot refusal machinery ADR-0082 §4 would have needed per callback, become compile errors or
  nothing.
- Good, because the registration gap, the clear-and-latch bookkeeping for N slots and the
  early-input queue of ADR-0213 decision 3 have nothing left to guard.
- Good, because "Capability is false iff `Unsupported`" becomes "the caller never sends a command
  the capabilities refuse", and the backdrop and the per-frame verbs stop failing silently.

### Confirmation

Written through the public API, from each crate's `tests/`, and each fails on the shape this
record removes. Host rows run through the conformance suite in `flui-testing` (ADR-0215): the
headless and winit hosts execute in CI, and a Windows, macOS, iOS or Android row is ✅ in
`docs/platform-parity.md` only after a run on that platform.

- `window_capabilities_are_honest`: for every host, each command whose capability is not
  `Unavailable` takes effect or is clamped, as the next snapshot shows, and each one whose
  capability is `Unavailable` is refused by `admit`. A capability change publishes a new revision
  and one `state_changed`.
- `admitted_command_has_one_constructor` (compile-fail pair in the contract crate's
  `tests/compile_fail/`): building an `AdmittedCommand` outside `admit` does not compile, and
  neither does passing a bare `WindowCommand` to `apply`.
- `a_command_admitted_before_a_capability_change_is_superseded`: a command admitted at one
  capabilities revision and applied after a capability change answers `ApplyError::Superseded`,
  not `Platform`, and admitting it again either applies it or is refused by `admit`.
- `submit_reports_a_refusal_and_a_closed_window`: `WindowControl::submit` answers
  `SubmitError::Admission` for a command the capabilities refuse and `SubmitError::Closed` once the
  window has closed.
- `window_facts_arrive_only_in_the_snapshot`: focus, visibility, hover, appearance, safe-area
  insets, execution state, display and metrics each publish a new revision followed by one
  `state_changed`, an equal sample publishes neither, and a non-drawable report publishes nothing.
- `window_handler_obligation_table`: one row per handler method. `input` returns the deferred forms
  for an event queued behind a reentrant callback and delivers the capability snapshot first;
  `state_changed` is payload-free and runs outside host locks; `close_requested` veto keeps the
  window open, proceed closes it; `closed` runs once; `surface_changed` releases before the handle
  dies and rebuilds unconditionally. A panicking method is contained and the next obligation is
  still delivered.
- `app_handler_obligation_table`: the same, one row per `AppHandler` method (ready, quit, reopen,
  session end with its answer, host notice, exit policy, wake deadline); a panicking method is
  contained with the first payload kept.
- `a_platform_window_cannot_cross_a_thread` (compile-fail): sending an `Rc<dyn PlatformWindow>`,
  an `Arc<dyn PlatformWindow>` or a `&dyn PlatformWindow` to another thread fails with `E0277`.
  The fixture passes only while the `Send` and `Sync` supertraits are gone.
- `a_surface_target_is_send_sync_and_unavailable_after_close`: a const assertion that the target is
  `HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`, and a conformance row that after
  `closed` it answers `HandleError::Unavailable` (Android: only for a terminated window), and that
  `pre_present_notify` is called once per presented frame, before the present, and returns at once
  on a host without the concept.
- `per_frame_verbs_report_unsupported_exactly_when_unavailable`: rows for the frame-rate hint,
  input regions and the backdrop, including a Windows build below the backdrop gate (injected
  version, as in ADR-0244's `floor_refusal_contract`) answering `Unsupported` with
  `NotOnThisDevice`.
- `unsupported_reaches_the_caller_unchanged`: a reason produced by a host arrives intact through
  admission, the verbs and `CursorError`.
- `platform_and_host_window_are_sealed` (compile-fail): an outside `impl Platform` or `impl
  HostWindow` fails to compile, while the `PlatformWindow` doctest still builds a window with no
  host in scope.
- `native_extensions_answer_none_on_a_foreign_host`: on the headless host and on a test
  double, every `flui::native::<os>` accessor answers `None` or `Unsupported` and none panics.

## Pros and cons of the options

### 1. The window contract decided above

- Good, because `WindowState` is the only source of window facts, and a consumer that needs to
  know which fact changed compares the new snapshot with the last one (decision 1).
- Good, because admission is a type: a host cannot receive an unvalidated command, the errors
  are split by who can cause them, and `Superseded` names the race between admission and
  application (decision 2).
- Good, because verbs the window cannot always honour answer in the same type, `Unsupported`,
  instead of logging, and OS versions never appear in the contract (decisions 3 and 6).
- Good, because one handler with a required method per obligation replaces the 14 `on_*`
  registrations: no inert defaults, no registration gap, no `Send` bound, one slot and one drop
  order (decision 4).
- Good, because the window is `!Send + !Sync` and the raster thread gets a separate `SurfaceTarget`,
  so a wrong-thread call stops compiling (decision 5).
- Good, because sealing `Platform` and `HostWindow` is reversible, `PlatformWindow` stays open with
  no inert defaults, and no second tier exists for first-party hosts (decision 7).
- Good, because the escape hatch is published, typed and total (decision 8).
- Good, because it keeps what is better than the references: the triad and its revision rule,
  owner-turn application of commands, `OwnerPlatform` as a `!Send` proof, `DispatchEventResult`,
  and `Since<F>` gates inside the host (decision 9).
- Bad, because every host, `flui-app` runner, test double and `Renderer::new` call site changes
  (see Consequences).
- Bad, because the admission token proves only that a command was validated against the
  capabilities read at admission; the OS keeps the last word, and the remaining test obligation is
  that a window's capabilities are honest (decision 2).

### 2. Keep the getters as conveniences beside the snapshot

- Bad, because each needs a conformance row that it agrees with the snapshot; two truths per fact
  is the failure ADR-0237 named.

### 3. State delivered as events with payloads (winit)

- Bad, because it is the same drift: a consumer reassembles state from events.

### 4. A desired state the host converges to (Slint)

- Bad, because it conflicts with changes the user or the OS makes and needs a conflict policy;
  ADR-0237 rejected it.

### 5. Per-capability typed handles (`window.maximizer() -> Option<..>`)

- Bad, because it multiplies the `Option` facets this record reduces.

### 6. Admission by convention plus a conformance row (today)

- Bad, because a host can receive an unvalidated command, and its `apply` carries two arms it
  must remember.

### 7. An `ApplyError` without `Superseded`

- Bad, because the race "admitted at one capabilities revision, applied at the next" lands in
  `Platform` and cannot be told from a failure of the OS; the caller could not know that admitting
  again is the answer.

### 8. Per-frame verbs and the backdrop through the command queue

- Bad, because commands apply at the owner's next turn, while the frame-rate hint and the input
  regions are sent every frame.

### 9. Keep `Send + Sync` and add owner-proof registrars (ADR-0082 §4 as written)

- Bad, because it removes `Send` from callbacks only; every other window method stays callable
  from any thread, documented instead of checked.

### 10. winit's cross-thread marshalling

- Good, because any thread can call any window method without an owner proof
  ([rust-windowing/winit@1893565716 winit-core/src/window.rs:620-631](https://github.com/rust-windowing/winit/blob/1893565716/winit-core/src/window.rs#L620-L631),
  [winit-core/src/application/mod.rs:10](https://github.com/rust-windowing/winit/blob/1893565716/winit-core/src/application/mod.rs#L10)).
- Bad, because it hides a blocking cross-thread call behind a method that looks cheap, and a
  deadlock is one owner-thread wait away; FLUI's lanes are bounded and typed (ADR-0039).

### 11. One event enum into one closure (Slint, Masonry)

- Bad, because three of the six obligations return different types (a dispatch result, a veto,
  nothing) with different delivery rules, so an enum needs a catch-all return.

### 12. A pull queue (SDL)

- Bad, because the deferred dispatch result needs a synchronous answer.

### 13. Keep the closures and make every `on_*` return `Result<(), Unsupported>`

- Good, because it fixes the inert defaults.
- Bad, because it fixes nothing else: the `Send` bound, the registration gap and the N slots stay.

### 14. A richer hidden tier for first-party hosts (Slint's `InternalToken`)

- Bad, because a third-party host becomes second class and cannot deliver touch or IME.

### 15. An open `Platform` and `HostWindow`

- Bad, because a new required method breaks every implementor; the references absorb that only
  because implementors share their repository.
- Bad, because an outside `Platform` could not mint `OwnerPlatform` anyway.

### 16. Per-OS fields in `WindowOptions` (GPUI) or untyped property keys (SDL)

- Bad, because the common struct grows per OS, or the values have no types.

### 17. winit's extension traits (downcast and `unwrap`)

- Bad, because they panic on a window of a foreign host.

### 18. An OS-version query in the contract

- Bad, because it lets an application write `if sdk >= 29`; ADR-0244 decision 11 forbids it.

## More information

- **Not implemented.** The code changes land as separate changes after this record, in the order
  of the feature's `tasks.md`; a platform's row in `docs/platform-parity.md` moves only after a
  run on that platform (see Confirmation).
