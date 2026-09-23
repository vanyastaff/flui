# ADR-0039: Event-loop affinity — a compile-time capability for main-thread platform operations

- **Status:** Accepted
- **Date:** 2026-07-28

## Context

`Platform` is `Send + Sync + 'static` with about thirty `&self` methods, and
nothing distinguished an operation any thread may perform (`background_executor`,
`name`) from one that is owner-thread-only on real operating systems
(`open_window`, `activate`, `displays`, `window_appearance`, `quit`). There was
no runtime check either. On macOS, `MacOSPlatform` was `unsafe impl Send/Sync`
"by AppKit convention" while `quit()` messaged `NSApp` from whatever thread
called it — UB-class off the main thread. On Windows, creating a window off the
message-loop thread silently binds its queue to the wrong thread.

Three pieces already pointed the right way:

1. **The winit owner-control lane** — a bounded FIFO with a closed command enum,
   per-request replies, typed rejection that returns the rejected payload
   (`Full`, `OwnerGone`), an admission gate that linearizes shutdown against
   in-flight sends, and a coalesced wake. Owner-thread callers got a typed
   `OwnerWouldBlock` instead of a deadlock.
2. **Realm dispatch rejects wrong threads** (`RealmDispatchError::WrongThread`),
   so everything reached through a realm already runs on the owner thread.
3. **ADR-0027/0037** — `UiRealm` is `!Send + !Sync`; cross-thread traffic is a
   closed, bounded, typed vocabulary, never closures.

Two forces shaped the answer. winit invoked `on_window_event` handlers while
holding its non-reentrant state mutex, so owner-thread code creating a window
directly from callback context either deadlocks or mutates the window map
mid-frame. And several first-party flows created windows before `run()` or
with no `run()` at all (Android and web bootstrap, native Win32 examples,
headless tests); each needed a home, not a deleted capability.

This is thread-topology design inside ADR-0027's leapfrog zone; Flutter is not
the reference.

## Decision

### 1. `OwnerPlatform` is the owner-thread capability

`OwnerPlatform` (`flui-platform`, `traits/owner.rs`) is minted **only by a
backend**, on the thread that owns — or, before the loop starts, will own —
its event loop, and handed to `on_ready`. It is `!Send + !Sync`, so possession
proves the thread at compile time. Loop liveness stays a runtime `Result`
(before the loop starts, after it exits).

It carries the operations that are owner-affine on at least one real OS and
implemented by at least one backend: `open_window`, `active_window`,
`displays`, `primary_display`, `activate`, `window_appearance`,
`keyboard_layout`, `quit`, `on_wake`; plus `shared()` and `proxy()`.

- `shared()` returns `SharedPlatform`, a `Clone + Send + Sync` wrapper exposing
  only the thread-safe surface (§2). Returning `&dyn Platform` would let a
  caller carry it across a thread boundary and call an owner-affine method in
  safe code; `SharedPlatform`'s method list is the fence.
- `OwnerPlatform` is not `Clone`. The runtime stores it once and lends
  `&OwnerPlatform` through a scoped accessor (§6).
- `Platform` is effectively sealed: an out-of-crate implementor cannot mint the
  `OwnerPlatform` its own `run()` must hand to `on_ready`. An external-embedder
  minting seam is separate design work (#560).

The ready callback is fallible and carries the capability by value:
`PlatformReadyCallback = Box<dyn FnOnce(OwnerPlatform) -> Result<(), BootstrapError>>`,
and `Platform::run` returns `Result<(), PlatformError>`, so a bootstrap failure
(window, GPU, root attach) stops the loop instead of leaving a half-built app
running. `on_ready` runs on the owner thread at the earliest point each backend
can create windows — before `app.run()` on macOS, before the message pump on
Windows, before the RAF loop on web, at `resumed` on winit, at the first
`Resume` on Android, immediately on headless.

**Window creation after bootstrap is deferred, never direct.**
`open_window` returns `WindowOpen::Ready(window)` or `WindowOpen::Pending(PendingWindow)`.
Inside `on_ready` it is always `Ready` (`try_ready()` converts). Afterwards an
owner-thread call enqueues on the owner lane without blocking and resolves at
the loop's next drain anchor, where the event loop is a parameter rather than a
TLS read. A call from any callback context therefore cannot re-enter the
backend's state lock or mutate the window map mid-frame. Errors are typed and
`#[non_exhaustive]`: `LaneFull { rejected }`, `OwnerGone { rejected }`,
`Backend`, `Unavailable`, `NotReady(PendingWindow)`.

### 2. What stays on `Platform`

`run`, `background_executor`, `capabilities`/`name`/`compositor_name`,
`app_path`, the `on_*` registrations (registration writes a `Send` callback;
delivery is owner-thread by construction), `clipboard()` and `data_transfer()`
(resolution is thread-safe; §5), and the shell dispatches
`open_url`/`reveal_path`/`open_path` (OS calls usable from any thread).

`prompt_for_paths`/`prompt_for_new_path` keep their seat unchanged. The one
real implementation (Windows) already runs its dialog on a dedicated STA
thread, and it returns `flui_platform::Task`, whose drop does not cancel; that
type must not spread onto a new capability. Consolidating prompts onto the
framework's async mechanism, with the macOS main-thread panel story, is its own
ADR.

`hide`, `hide_other_apps`, `unhide_other_apps`, `should_auto_hide_scrollbars`
and `window_stack` were deleted: no backend implemented them for real and
nothing called them. Each returns, on `OwnerPlatform` if owner-affine, with its
first real implementation and consumer.

### 3. `PlatformProxy`: workers reach the owner

`PlatformProxy` (`Clone + Send + Sync`) generalizes the winit lane into a
backend-agnostic capability. It never blocks the sender and never carries
closures:

- `open_window(options) -> Result<PendingWindow, ProxySendError<WindowOptions>>`
- `request_quit()` — a coalesced flag that bypasses queue capacity, so
  shutdown never waits behind backpressure
- `wake()` — a window-independent owner turn (§7)
- `is_owner_thread()` — diagnostic only; correctness rests on the types

`ProxySendError` is a closed vocabulary: `Full { capacity, rejected }`,
`OwnerGone { rejected }` (a lane existed and its loop died), `Unsupported
{ rejected }` (this backend has no lane for this request — permanent, do not
retry), and `WakeFailed { rejected, source }`. The rejected value is always
returned, so a producer can retry without rebuilding it (ADR-0027 §4).

**The reply is a claim slot, not a buffered send.** A buffered one-shot
succeeds whenever the receiver handle is alive, even if nobody reads the
reply, so a requester that drops its handle after the send leaks the window.
`ClaimSlot<T>`/`ClaimHandle<T>` (`flui-foundation`) hold the state machine:

```text
Pending   ──owner delivers──────▶ Delivered(result)
Pending   ──requester drops─────▶ Abandoned            (owner skips creation)
Delivered ──requester claims────▶ Claimed              (requester owns the window)
Delivered ──requester drops─────▶ Abandoned(window)    (owner closes and unregisters it)
Pending   ──owner side dropped──▶ OwnerGone            (waiters resolve, never hang)
```

The slot is the at-most-once linearization point. A dying realm or finished
worker that drops its `PendingWindow` cannot leak a window in any ordering.
`PendingWindow` offers `wait()` (worker threads only — on the owner thread it
returns `WaitError::WouldBlockOwner` with the handle, because waiting there
would deadlock the lane the caller drains), `try_take()`, and a `Future` impl
for polling at `Idle` through the framework's async driver.

The claim slot lives in `flui-foundation` so its tests run with the
foundation suite.

### 4. Protocol rules

- **Ordering.** One bounded FIFO per owner, drained from a pre-read snapshot, so
  a drain is finite and FIFO per batch. `Quit` is a flag and may overtake queued
  window requests. A posted programmatic window close (#919) is never
  overtaken: the owner runs every posted close's teardown before exiting, so a
  quit cannot strand a hidden-but-tracked window.
- **Drain anchors.** On winit the lane drains only from `user_event` at the top
  of the loop, never inside a nested callback. On AppKit and Win32 wakes are
  dispatched inside nested modal run loops (alerts, modal dialogs,
  `DoDragDrop`), so each such backend must gate its drain: no drain while a
  nested/modal dispatch region or a frame transaction is active, and a wake
  that arrives then re-arms for the next top-level anchor. Each backend's lane
  adoption carries a test that a wake during a nested modal loop defers.
- **Stale results.** Lane replies never commit mid-frame; they enter realm
  state at Idle commit points, where each work class applies its own freshness
  check (ADR-0027 §6). A window opened for a realm that died meanwhile is
  reclaimed through the abandoned slot.
- **Window identity.** `WindowId` stays the platform-internal native key;
  realm-facing identity is the generational `PresentationAddress`
  (ADR-0037 §2). winit's ids are monotonic. The macOS backend's pointer-as-id
  is an ABA hazard to replace with a monotonic mint before multi-window
  sessions on that backend.

### 5. `Clipboard` stays `Send + Sync`; macOS routes internally

`Clipboard: Send + Sync` is unchanged and carries no affinity assertion.
`NSPasteboard` is **not** safe off the main thread (Apple's thread-safety
summary lists no exception, and the macOS clipboard tests crashed under
parallel execution), so `MacOSClipboard` dispatches every operation to the
AppKit main thread through the main dispatch queue. The lane enforces the
ordering; an assertion would only panic legitimate cross-thread callers. Rich
pasteboard items and promised data are the data-transfer design's to place
(ADR-0038).

### 6. Composition with the runtime

- **The capability is loop-scoped, not realm-scoped.** `AppRuntime` holds it
  from `on_ready` until `run` returns (on macOS `run` never returns). Realm
  teardown does not clear it: the loop may host another realm before it exits,
  as hot restart does.
- **Scoped, fenced access.** `with_owner_platform` is `pub(crate)` to
  `flui-app`, never re-exported. It clones the internal `Rc<OwnerPlatform>`,
  ends the runtime borrow, then passes `&OwnerPlatform` to a closure — creating
  or revealing a native window can synchronously deliver focus or close
  callbacks back into the runtime, so no borrow may be held across it. It
  refuses to run during `TransientCallbacks`, `MidFrameMicrotasks` or
  `PersistentCallbacks` (Idle and post-frame work are allowed). A controller
  re-checks its loop identity and quit admission after native calls, and
  closes an obsolete result instead of publishing it.
- Widget-tree access to platform operations, if ever needed, is a lifecycle
  capability (ADR-0078).

### 7. Owner turns without windows, and a resident main window

**Owner signals.** `OwnerPlatform::on_wake` installs an owner-thread callback;
workers hold only `PlatformProxy::wake` and `request_quit`. A shared pump
coalesces bursts, runs callbacks outside locks, runs re-entrant wakes on a
later turn, and contains callback and cleanup panics at the native boundary.
Quit immediately fences wake and window admission. AppKit wakes through GCD;
Win32 through a dedicated message-only window and class (not the visible
windows' procedure); headless exposes an owner-local manual driver; mobile and
web report registration as unsupported.

**Pending secondary windows belong to the loop**, not to the first realm's
async driver. Liveness is reserved before native creation; ready requests are
polled in finite batches outside TLS borrows; a shared request captures an
exact `RealmId`. Failures release reservations once and close resolved,
uninstalled windows. A failed physical wake is traced and cancels the request;
nothing spins.

**Resident main window.** `Application<V, F>` separates application lifetime
from a root tree: an owner-local `FnMut(&AppHandle) -> V` factory builds fresh
view state per window. `StartupWindow::None` starts services without fonts or
a GPU; `ExitPolicy::ExplicitQuit` keeps an empty loop alive (last-window exit
stays the default). `AppHandle` sends only show and quit intents; a
`MainWindowRequest` reply uses a claim slot for the result, not for native
resource ownership, so dropping the receiver does not cancel an accepted show.
Show requests admitted during disposal reserve the next generation; quit
cancels active and queued generations. A successful reply proves installation
and a redraw request, not GPU presentation. Renderer, realm, root and input
belong to each window; clipboard, execution services, exit hooks and the asset
watcher belong to the loop. The shape follows Iced's daemon and GPUI's
owner-context window creation, with a closed command vocabulary instead of
cross-thread UI closures.

## Implementation status

Built: `OwnerAffinity` debug assertions on macOS and Windows; `OwnerPlatform`,
`SharedPlatform`, `PlatformProxy`, `PendingWindow` and the claim slot; the
fallible ready callback on every backend; owner signals; loop-owned pending
windows; the resident main window. Not yet: moving the §1 methods off
`Platform` into a `pub(crate)` owner-operations trait (after which
`MacOSPlatform`'s `unsafe impl Send/Sync` covers only the residual surface),
and owner lanes behind `PlatformProxy::open_window` on backends that report
`Unsupported`. That move waits on on-device validation of the Android
bootstrap inside `on_ready`.

## Consequences

- Wrong-thread platform calls are unrepresentable for capability holders on
  every backend, including the two CI only type-checks.
- Owner-thread window creation after bootstrap works without a blocking or
  re-entrant path; callers must handle `Pending`.
- A dropped requester can no longer leak a window; the orphan may flicker open
  and closed instead, a release-notes item.
- The ready-callback signature broke every backend and embedder once, pre-1.0.
  The pending trait move will break external `Platform` implementors again.

## Alternatives rejected

- **Runtime assertions only.** Catches a violation once, in a debug build, on
  the affected OS — and those OSes are exactly the ones CI does not execute.
- **A lifetime-bound `ActivePlatformContext<'loop>`.** Dies when `on_ready`
  returns, but most owner work happens later in platform callbacks.
- **Marshaling closures to the main thread (a foreground executor).**
  Closures erase what crosses the boundary, defeating bounded typed
  backpressure and freshness gating (ADR-0027 §9, ADR-0037 §3).
- **Making `Platform` itself `!Send`.** Forbids legitimately cross-thread
  operations: executor access, callback registration, cross-thread quit,
  clipboard resolution.
- **Transparent enqueue-and-block inside every method.** Hides a rendezvous in
  innocuous calls, deadlocks when the owner calls, and turns backpressure into
  an invisible stall.
- **Messages for everything, including bootstrap and reads.** Bootstrap must be
  direct, and owner-thread reads would pay a queue round trip for data one call
  away.
- **Direct owner-thread creation by widening winit's event-loop TLS to every
  callback.** Deadlocks silently when a handler invoked under the state mutex
  creates a window, and lets frame code mutate the window map mid-frame.
