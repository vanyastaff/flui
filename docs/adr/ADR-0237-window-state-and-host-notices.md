# ADR-0237: A window publishes a state snapshot and takes commands; the host delivers system notices

- **Status:** Accepted
- **Date:** 2026-10-18
- **Amended:** 2026-10-19 — decision 9's image cache is the renderer's image textures; decoded
  images belong to the application's asset registry, reclaimed once registered
  ([ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md)).
- **Superseded-by:** [ADR-0254](ADR-0254-window-contract.md), in part: the window-scoped getters
  and per-fact callbacks that sit beside the snapshot (decision 1 keeps `PlatformWindow` as the
  one window trait, but it no longer carries them); decision 3's refusal inside `apply` (it moves
  to `WindowCommand::admit`, which yields the only value `apply` takes; "refused exactly when the
  capability is false" stays); and the registrations that deliver decisions 7 and 8's notices
  (`Platform::on_host_notice`, `Platform::on_session_end`), which become `AppHandler` methods. The
  snapshot with its revision, the command and capability triad, owner-turn application and
  `WindowHandle` (decision 4), placement, and the notices and the session-end hold themselves
  stay; the payload-free state notification stays as a handler method instead of a registration.
- **Related:** [ADR-0222](ADR-0222-window-appearance-is-a-presentation-observation.md) (the
  payload-free observation shape this follows),
  [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) (display snapshot and ids; this ADR
  adds the persistent display key ADR-0236 decision 3 deferred),
  [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) (notices sit next to the
  lifecycle state, not inside it), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  (capabilities on `LifecycleContext`), [ADR-0123](ADR-0123-exceptional-presentation-close.md)
  (close stays the vetoed close path), [ADR-0082](ADR-0082-platform-api-contract-crate.md).
- **Spec:** [window-lifecycle](../plans/specs/window-lifecycle/design.md)

## Context

App code had no way to change its window after creation: the title, position, size, constraints
and mode were configurable only through `AppConfig`, and part of that was lost converting to
`WindowOptions`. `PlatformWindow` carried a dozen per-feature methods whose defaults looked like
answers (`false`, `Bounds::default()`, an empty title), so "unsupported" and "done" were
indistinguishable and state readback was fiction on macOS and Linux. A second window trait
(`flui_platform::window::Window`) with its own builder and manager existed beside it, reachable from
nothing. Windows ignored the size constraints and `resizable`, and both Windows and X11 showed the
window before its first frame.

System events were worse: `Platform::on_session_end` was declared but implemented by no backend and
not exposed through `OwnerPlatform`, so `flui-app` could not register it; there was no contract for
sleep, wake, session lock or memory pressure; Android derived the lifecycle from window focus; the
Web ignored page freeze and GPU context loss.

## Decision

1. **One window trait.** `PlatformWindow` is the only window trait. The second trait, its builder
   and manager, the old `WindowMode`/`WindowBounds`, and the per-feature setters and getters (title,
   minimize, maximize, restore, fullscreen, resize, bounds, maximized and fullscreen queries, the
   moved callback and the mode variants of `WindowEvent`) are removed.
2. **State is an immutable snapshot with a revision.** `PlatformWindow::state()` returns a
   `WindowState` value (geometry in logical pixels, mode, normal geometry, visibility, focus, title,
   constraints, flags, display) that describes what the OS reports, not what code asked for.
   Position is `None` where the OS does not report a global position. A change publishes a new
   snapshot with a higher revision and one payload-free `on_state_changed` notification; an equal
   sample keeps the revision. An exhausted revision refuses permanently. Backends share one cell
   (`WindowStateCell`) built like ADR-0236's `DisplaySource`. The snapshot is also how a move
   reaches the framework: a published state whose position, display or frame differs from the
   previous one is the window moving, and the runtime re-reads its gesture geometry (ADR-0212 §2)
   and re-sends its input regions (ADR-0217 §5) before any state subscriber runs. An input
   recording keeps that as a `Moved` observation, never the payload-free notification.
3. **Changes are commands checked against capabilities.** Every change is a `WindowCommand` applied
   through `PlatformWindow::apply`. Each window reports `WindowCapabilities`; `apply` refuses with
   `Unsupported` exactly when the corresponding capability is false, so an app can ask up front
   and is never told "done" for a no-op. Invalid geometry is refused before the OS is called. An
   accepted command is a request: its outcome is visible only in the next snapshot. Fullscreen is
   borderless on the window's display; exclusive mode changes are not part of the contract.
4. **App code reaches a window through `WindowHandle`.** `LifecycleContext::window()` (never
   `BuildContext`) and the window-open request hand out a weak, owner-local handle. Commands are
   validated immediately, queued, and applied on the owner thread at the start of its next turn,
   before the frame, in submission order. Applying them later rather than inline keeps a native
   call that synchronously re-enters the window procedure out of user code. After the window closes
   the handle answers `Closed`. The window-open request delivers the new window's
   `PresentationAddress`, not the handle: a `WindowHandle` is `!Send` and the request travels
   through a `Send` future, so the owner thread turns the address into a handle with
   `flui_app::window_handle(address)`. The window can close between the request's completion and
   that lookup; the lookup then returns `None`, as it does on another thread or for an unknown
   address, and the caller treats `None` as a closed window.
5. **Windows open revealed by their first frame.** Runner-opened windows are shown only after the
   first presented frame, already in their initial mode, with the existing bounded fallback.
   `WindowReveal::AtOpen` stays an explicit choice for direct platform users.
6. **Placement is FLUI's own value.** `WindowPlacement` stores a persistent display key, the normal
   geometry relative to that display's usable area in its logical pixels, and the mode, in a
   versioned encoding. Resolution against the current display snapshot is a pure function that
   always yields a window wholly on a live display. `DisplayInfo` gains `persistent_key`, a string
   that survives reboots (the monitor device path on Windows, not the LUID), compared only for
   equality. OS autosave facilities are not used, so behaviour is the same everywhere.
7. **System notices are a separate stream.** The platform reports `SystemNotice` (the same events without a session end, which has its own hook); `HostNotice` (session ending, cancelled, ended; sleep;
   wake; session locked and unlocked; memory pressure at `Moderate` or `Critical`) is delivered by
   `OwnerPlatform` to `flui-app`, which fans it out to every presentation's `LifecycleHandle`
   (`subscribe_notices`, same FIFO and containment rules as lifecycle subscriptions).
   `AppLifecycleState` is unchanged; page freeze maps onto the execution state, not a notice.
   `WidgetsBindingObserver`'s memory-pressure and exit-request hooks are removed.
8. **Session end can be held only where the OS allows it.** A subscriber may take a
   `SessionEndBlock` with a reason; while any block lives the OS is answered "wait"
   (`ShutdownBlockReasonCreate` on Windows, `NSTerminateLater` on macOS); dropping the last block
   releases it. Elsewhere the notice is informational. On session end the framework saves window
   placement and waits for accepted storage writes within a bounded budget before letting the OS
   proceed. A panicking subscriber counts as "proceed". The query is answered only after every
   subscriber has run: when a runtime's turn is busy (a session end arriving inside another
   callback) the answer is "proceed" with a diagnostic. The answer closes the request's book, so a
   later block is refused (`SessionEndBlockError::Answered`), and dropping the last block after a
   "wait" answer reaches the platform through `Platform::release_session_end`.
9. **The framework reclaims its own caches first.** On memory pressure the host reclaims the image
   cache, the text service's buffers and the renderer's pools (on the raster thread) according to the
   level, then notifies subscribers. Repeated notices of one level within one owner turn coalesce.
10. **GPU context loss is a platform event.** Where the platform reports loss (WebGL context lost,
    WebGPU device lost), it marks the renderer lost and wakes the owner so recovery starts at once
    rather than after the next frame.

## Consequences

- Every backend implements `state`, `capabilities` and `apply`; the conformance suite in
  `flui-testing` holds each one to "capability false ⇔ `Unsupported`" and to snapshot convergence.
- Callers of the removed methods (runner, first-reveal policy, examples, hot reload) move to the
  snapshot and commands in the change that introduces them.
- Android has no `onTrimMemory` without Java code; its pressure source is `onLowMemory` plus the
  process trim level read at lifecycle transitions. Linux notices need logind and PSI; without
  them the sources stay silent, with one diagnostic.
- Wayland cannot report minimized state or global position; the capabilities say so instead of a
  default that pretends.

## Alternatives rejected

- **Add the missing setters to the existing trait.** Keeps defaults that masquerade as answers and
  a state assembled from six events that drift from readback.
- **State as a reactive "desired" signal the backend converges to.** Conflicts with changes the
  user or OS makes (the user maximizes; desired says normal) and needs a conflict policy no other
  toolkit needs.
- **OS placement autosave (`frameAutosaveName`, registry).** Different behaviour per OS and no
  validation against displays that disappeared.
- **Shipping a dex for Android `ComponentCallbacks2`.** Against the no-Java stance ADR-0236's spec
  already took for display listeners.
