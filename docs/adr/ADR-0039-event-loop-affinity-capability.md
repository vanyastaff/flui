# ADR-0039: Event-loop affinity — a compile-time capability for main-thread platform operations

*Owner-thread-only platform operations move off the `Send + Sync` `Platform` trait onto `OwnerPlatform` — a `!Send + !Sync` capability value minted by a backend on the thread that owns (or will own) its event loop and handed to `on_ready`. Owner-thread window creation after bootstrap is **deferred, never direct**: it enqueues on the owner lane and resolves at the loop's next drain anchor, so it can neither deadlock on the backend's own state lock nor mutate the platform window map mid-frame. Worker threads reach the owner through `PlatformProxy`, a generalization of the winit backend's existing owner-control lane (bounded FIFO, typed `Full`/`OwnerGone` errors, claim-slot replies with at-most-once effect). Slice 1 is a runtime debug-assert backstop whose testable primitive lands in flui-foundation (so CI actually runs its tests); the trait surgery is a later slice.*

---

- **Status:** Proposed (2026-07-28)
- **Date:** 2026-07-28
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-foundation/src` (new `OwnerAffinity` primitive — the slice-1 CI-tested deliverable), `crates/flui-platform/src/traits/platform.rs` (trait split), new `crates/flui-platform/src/traits/owner.rs` (`OwnerPlatform`, `PlatformProxy`, `PendingWindow`), `crates/flui-platform/src/platforms/winit/{platform,control}.rs` (lane generalization + handler-dispatch hoist), `crates/flui-platform/src/platforms/{macos,windows}/platform.rs` (slice-1 asserts), `crates/flui-app/src/app/{runner,direct}.rs` (bootstrap migrations + owner-TLS host), `examples/*` (on_ready migration)
- **Related:** ADR-0027 (owner-affine UI realms — the sanctioned leapfrog zone this design lives in), ADR-0037 (presentation ownership domains — `PresentationId`, closed cross-thread vocabulary), ADR-0034 (clipboard reachability; named the macOS pasteboard-affinity hazard — §5 supersedes its suggested assert), audit 2026-07-25 §23 (U7)
- **Numbering note:** `docs/adr/` ends at 0037 at HEAD; this number assumes the in-flight 0038 draft lands first. Renumber at landing if it does not.

---

## Context

### The defect

`Platform` is declared `Send + Sync + 'static` with ~30 `&self` methods (`crates/flui-platform/src/traits/platform.rs:171`), and its own doc-comment advertises the design as "Interior mutability: Implementations use Mutex/RwLock for thread-safe &self methods" (`platform.rs:157-158`). Nothing in the trait distinguishes an operation that any thread may perform (`background_executor`, `name`) from one that is owner-thread-only on real operating systems (`open_window` `:209`, `activate`/`hide` `:240-251`, `displays`/`primary_display` `:224-227`, `window_appearance` `:256`, `quit` `:200`).

There is no runtime backstop either: a search for `main_thread|is_main_thread|MainThread|pthread_main|IsGUIThread|NSThread` across `crates/flui-platform/src` and `crates/flui-app/src` returns **zero matches** (re-verified at HEAD, 2026-07-28). The macOS backend makes the hazard explicit: `MacOSPlatform` is `unsafe impl Send`/`Sync` justified only by "AppKit convention" (`platforms/macos/platform.rs:46-53`), yet `quit()` sends `terminate:` to `NSApp` from whatever thread calls it (`:142-148`), `active_window()` messages `keyWindow` (`:156-168`), and `open_window()` constructs an `NSWindow` (`:150-154`). Calling AppKit off the main thread is UB-class; on Windows, window creation off the message-loop thread silently binds the window's message queue to the wrong thread.

### Corrections to the audit (verified at HEAD)

The audit's §23 recommendation was written against code that has since moved:

- **`foreground_executor()` no longer exists.** The audit proposed building the command channel "on top of the existing `foreground_executor`" — that method was removed (commits 379d6944/493a21a2); a grep for it returns zero matches. The substrate that *does* exist is better: the winit backend's owner-control lane (`platforms/winit/control.rs`), described below.
- **`Arc<dyn Platform>` sharing no longer exists as an app-facing shape.** `flui_platform::current_platform()` returns `anyhow::Result<Box<dyn Platform>>` (`lib.rs:316`); `Platform::run(self: Box<Self>, on_ready)` consumes it (`traits/platform.rs:194`), handing `on_ready` a `&dyn Platform` (`PlatformReadyCallback`, `:142`). The escape hatches through which platform-shaped objects reach other threads have narrowed to `Arc<dyn Clipboard>` (stashed in `AppBinding`, `binding.rs:110`, per ADR-0034) and `Arc<dyn PlatformExecutor>` (`platform.rs:177`).
- **`set_cursor_style` no longer exists.** Cursor selection became window-scoped 2026-07-23 (ADR-0034, "Resolved follow-up"); it needs no seat in this design.

### What already points the right way

Three pieces of the answer are already in the tree; this ADR generalizes them rather than inventing a parallel mechanism:

1. **The winit owner-control lane.** `ControlSender`/`ControlReceiver` (`platforms/winit/control.rs`) is a bounded (capacity 256, `:16`) crossbeam FIFO carrying a closed command enum (`ControlCommand::OpenWindow`, `:21-26`) with a per-request one-shot reply, typed rejection that **returns the rejected payload** (`ControlSendError::{Full, OwnerGone}`, `:39-48`), an admission gate that linearizes shutdown against in-flight sends (`:57-58`, `:198-203`), and a coalesced wake (`:159-165`). The receiver is `!Send + !Sync` by `PhantomData<Rc<()>>` (`:80`, statically asserted `:229-230`). `WinitRunState` records `owner_thread: ThreadId` (`platforms/winit/platform.rs:90-103`) and `control_for_open_window(caller_thread)` returns typed state errors including `OwnerWouldBlock` for owner-thread callers (`:106-123`). `open_window` uses a same-thread fast path through the `ACTIVE_EVENT_LOOP` TLS publication during `on_ready` (`:427-457`, `:901-922`) and marshals cross-thread requests through the lane otherwise (`:924-970`); the owner drains with a bounded pre-read snapshot and completes replies via a panic-safe guard (`:785-816`, `:471-497`).
2. **Realm dispatch already rejects wrong threads.** `RealmDispatcher { owner_thread, realm_id }` (`flui-app/src/app/runner.rs:143-146`) and `dispatch_platform_realm`'s first check — `RealmDispatchError::WrongThread` (`:847-850`) — mean everything reached via realm dispatch already runs on the owner thread. Desktop bootstrap `debug_assert`s the same (`:2161-2165`).
3. **The ADR-0027/0037 model.** `UiRealm` is `!Send + !Sync` (ADR-0027 §2); cross-thread traffic is a closed, bounded, typed vocabulary — never closures (ADR-0027 §4/§9, ADR-0037 §3); `WindowId(u64)` stays the platform-internal native key while realm-facing identity is generational (`RealmId`, `PresentationId`; ADR-0027 §6, ADR-0037 §2). This ADR is process/thread-topology design inside the sanctioned leapfrog zone — Flutter is not the reference here.

### Forces

- A runtime check catches a violation once, in a debug build, on the OS where it bites; a `!Send` type makes the violation unrepresentable on every OS, including the two we only cross-typecheck (CI's cross-typecheck job is the *only* gate on the Win32/AppKit backends — no tests run there).
- The owner thread must never block on its own lane (`OwnerWouldBlock` exists because winit deadlocked otherwise — module doc `platforms/winit/platform.rs:18-30`).
- **The winit backend dispatches `Platform::on_window_event` handlers while holding its non-reentrant `parking_lot` state mutex** (`handlers.invoke_window_event` inside `with_state` closures: `platforms/winit/platform.rs:558-563`, `:598-603`, `:614-620`, `:632-645`, `:650-657`; `with_state` locks at `:289`), while per-window `win.callbacks()` dispatch happens *outside* that lock (`:554`, `:594`, `:610`, `:628`, `:668`). Any design that lets owner-thread code create windows directly from arbitrary callback context either deadlocks on that lock or mutates the platform window map mid-frame — this force killed the first draft of this ADR (see Alternative G).
- **Several first-party flows create windows before `run()` — or without any `run()` at all.** Pre-run: Android bootstrap (`runner.rs:2305-2307`), web bootstrap (`runner.rs:2620-2624`, "before run() since run() takes ownership"), `run_direct` (`direct.rs:101` — its own comment already documents it as broken on winit and needing the on_ready reorder `run_desktop` received), and the native Win32 examples (`examples/windows11_demo.rs:54`, `windows11_features.rs:77`, `test_background.rs:36`, `window_features.rs:59`). Run-less: headless tests construct `headless_platform()` and open windows with no event loop ever started (`binding.rs:3997`, `ui_realm.rs:748`, `runner.rs:2965-2977`). All run on the thread that owns (or would own) the loop; the design must give each a home, not delete their capability (see slices 2–3).
- SP-6: no locks or raw channels in public API. Trigger #22: no lifecycle capability acquired during build/layout/paint. `TaskToken` (`flui-scheduler/src/async_driver.rs:135-171`) is the framework's one async mechanism; `flui_platform::Task` is a flagged parallel definition (`task.rs:74`) this design must not entrench.

## Decision

### 1. `OwnerPlatform` — the owner-thread capability

A new public type in `flui-platform`, minted **only by a backend**, on the thread that owns — or, before the loop starts, *will* own — its event loop, and delivered to `on_ready`. `!Send + !Sync` by phantom marker, so the compiler — not a debug assert — guarantees every call happens on that thread. Possession proves the **thread**; loop liveness stays a runtime `Result` in both directions (before the loop starts, after it exits — the winit `OpenWindowStateError` taxonomy, `platforms/winit/platform.rs:83-88`, already models this):

```rust
// crates/flui-platform/src/traits/owner.rs
use std::{marker::PhantomData, sync::Arc};

/// Owner-thread platform capability. Minted only by a backend (`pub(crate)`
/// constructor) on the thread that owns or will own its event loop;
/// `!Send + !Sync`, so possession proves execution on that thread. Carries
/// exactly the operations that are (a) owner-affine on at least one real OS
/// and (b) implemented by at least one real backend today — no dead surface.
pub struct OwnerPlatform {
    platform: Arc<dyn Platform>, // slice 2; shrinks to Arc<dyn OwnerOps> in slice 3
    _owner_affine: PhantomData<*const ()>, // !Send + !Sync
}

impl OwnerPlatform {
    // Window management. Deferred-capable: `Ready` is guaranteed inside
    // `on_ready`; afterwards the backend may resolve via its owner lane at
    // the next drain anchor (§3). Never blocks; never runs inside a nested
    // callback or while backend state locks are held.
    pub fn open_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError>;
    pub fn active_window(&self) -> Option<WindowId>;
    // Displays
    pub fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>>;
    pub fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>>;
    // App activation / appearance / keyboard (implemented on ≥1 backend today)
    pub fn activate(&self, ignoring_other_apps: bool);
    pub fn window_appearance(&self) -> WindowAppearance;
    pub fn keyboard_layout(&self) -> String;
    // Lifecycle
    pub fn quit(&self);
    // Escape to the residual thread-safe surface (§2)
    pub fn shared(&self) -> &dyn Platform;
    // Cross-thread capability mint (§3): handed to workers, realms, tasks
    pub fn proxy(&self) -> PlatformProxy;
}

/// Result of an owner-thread window-open request.
pub enum WindowOpen {
    /// Created synchronously. Always the case inside `on_ready` (every
    /// backend), and on backends whose owner thread may create directly
    /// outside callbacks.
    Ready(Arc<dyn PlatformWindow>),
    /// Enqueued on the owner lane; resolves at the loop's next drain anchor.
    Pending(PendingWindow),
}

impl WindowOpen {
    /// Bootstrap convenience: unwrap `Ready`; typed error otherwise.
    /// Guaranteed to succeed inside `on_ready`.
    pub fn expect_ready(self) -> Result<Arc<dyn PlatformWindow>, OpenWindowError>;
}
```

Errors are typed (`thiserror`, house rule for libraries — the existing trait's `anyhow` surface is left as-is until slice 3, at which point the *moved* methods adopt the typed taxonomy):

```rust
#[derive(Debug, thiserror::Error)]
pub enum OpenWindowError {
    #[error("owner lane is full (capacity {capacity})")]
    LaneFull { capacity: usize, rejected: WindowOptions },
    /// `rejected` is `Some` when refusal happens at enqueue (ADR-0027 §4:
    /// the producer can retry without rebuilding options) and `None` when
    /// the loop died after the request was already accepted.
    #[error("event-loop owner is gone")]
    OwnerGone { rejected: Option<WindowOptions> },
    #[error("the backend could not create the window: {message}")]
    Backend { message: String },
    #[error("window creation was deferred; this call site requires Ready")]
    NotReady(PendingWindow),
}
```

`Platform::run`'s callback widens to carry the capability by value:

```rust
/// Replaces `PlatformReadyCallback = Box<dyn FnOnce(&dyn Platform)>` (platform.rs:142).
pub type PlatformReadyCallback = Box<dyn FnOnce(OwnerPlatform)>;
```

`on_ready` may stash the capability in owner-thread state for the rest of the loop's life (it is not lifetime-bound; see Alternative B). Per-backend `on_ready` timing is part of the contract and varies: macOS invokes it before `app.run()` (`macos/platform.rs:126` → `:138`), Windows before the message pump (`windows/platform.rs:896-903`), web before starting the RAF loop (`web/platform.rs:123-133`), winit at `resumed`, Android at the first `Resume` (`android/mod.rs:13`, `:172`), headless immediately — and headless's `run` then *returns* (`headless/platform.rs:97-105`), so "for the loop's life" on headless means "until the value is dropped". In every case `on_ready` runs on the owner thread at the earliest point the backend can create windows, which is why the pre-run bootstrap flows can migrate into it without losing capability (slice 2).

**How the winit backend powers it — no TLS widening.** The `ACTIVE_EVENT_LOOP` publication keeps exactly its current scope: during `on_ready` only (`platforms/winit/platform.rs:427-457`). Inside `on_ready`, `open_window` creates directly and returns `Ready` (today's fast path, `:901-922`). After bootstrap, an owner-thread `open_window` **enqueues on the owner lane without blocking** and returns `Pending`; the request resolves at the next `user_event` drain (`:774-776`, `:785-816`), where a live `ActiveEventLoop` is a parameter, not a TLS read. This is the load-bearing design choice: the owner thread gets *deferral*, not direct creation, so a call from any callback context — including a handler the backend invokes, or frame code — cannot re-enter the backend's state lock and cannot mutate the platform window map mid-frame. The first draft of this ADR instead widened the TLS publication to every handler callback; that design silently deadlocks and is rejected as Alternative G.

### 2. What stays on `Platform` (`&self`, `Send + Sync`) — and what is deleted

| Stays | Why it is genuinely thread-safe |
|---|---|
| `run(self: Box<Self>, on_ready)` | consumes the value; the affinity story starts inside it |
| `background_executor()` (`platform.rs:177`) | thread-pool handle; `PlatformExecutor: Send + Sync` (`:462`) unchanged |
| `capabilities()`, `name()`, `compositor_name()` (`:352-360`) | immutable descriptors |
| `app_path()` (`:363`) | pure environment read (macOS `NSBundle` path lookup is documented thread-safe; re-verify per backend at implementation time) |
| `on_quit`/`on_reopen`/`on_window_event`/`on_open_urls`/`on_keyboard_layout_change` (`:327-347`) | registration writes a `Send` callback into mutex-held handler storage; *delivery* is owner-thread by construction |
| `clipboard()` (`:232`) | resolution stays callable anywhere per ADR-0034's bootstrap shapes (Android/web resolve it pre-`run()`, `runner.rs:2290-2297`); see §5 |
| `open_url`/`reveal_path`/`open_path` (`:283-296`) | shell dispatch to the OS, not owner-affine: real bodies use `ShellExecuteW` (windows, `:1047-1100`), process spawn (winit, `:1043-1080`), `window.open` (web, `:217-221`) — all callable from any thread. Zero in-repo consumers is a debt note, not this ADR's concern |
| `prompt_for_paths`/`prompt_for_new_path` (`:302-317`) | **unresolved here, deliberately.** The one real implementation (Windows) already runs its COM dialog on a dedicated STA thread via the background executor (`windows/platform.rs:1102-1240`) — not owner-affine as implemented. It returns `flui_platform::Task`, the flagged parallel async definition (`task.rs:74`) whose drop does not cancel (`:107-110`); this ADR must not entrench that type on a new capability, so prompts keep their current seat *unchanged* and their consolidation — async shape (claim-slot pending reply vs. `TaskToken`) plus the macOS main-thread-panel affinity story — **is its own follow-up ADR** |
| `write_to_clipboard`/`read_from_clipboard` (`:268-279`) | default delegation to `clipboard()` plus a headless test body; zero consumers. Owned by the data-transfer design (§5) — untouched here |

In slice 3 the §1 method list leaves the trait, and `quit` additionally keeps a proxy verb — the winit backend already implements cross-thread quit as a coalesced flag through the lane (`control.rs:149-157`), which is exactly the proxy shape.

**Deleted outright in slice 3, not moved:** `hide`, `hide_other_apps`, `unhide_other_apps`, `should_auto_hide_scrollbars`, `window_stack`. The first four are default stubs with zero backend bodies and zero callers (`platform.rs:244-251`, `:260-263`); `window_stack` has zero consumers and no real-OS implementation — the trait default is `None` (`:217-219`), winit explicitly returns `None` ("not easily supported", `winit/platform.rs:976-978`), macOS/Windows inherit the default, and only the headless test backend and web return toy values. ADR-0034's "What is deferred" discipline — no surface ahead of a real implementation *and* consumer — says these die rather than ride along; each returns (on `OwnerPlatform` if owner-affine) with its first real implementation and consumer.

### 3. `PlatformProxy` — workers reach the owner thread

The winit `ControlSender` generalizes into a public, backend-agnostic capability. The lane implementation moves from `platforms/winit/control.rs` to `shared/owner_lane.rs`; each backend supplies the wake hook (winit `EventLoopProxy`; AppKit `CFRunLoopSource`; Win32 `PostMessageW` to a message-only HWND; headless flag+pump — the wake contract ADR-0027 §3 already fixed) **and a drain anchor that satisfies the gate contract in §4**:

```rust
// crates/flui-platform/src/traits/owner.rs
/// Cross-thread request capability. `Clone + Send + Sync`. Enqueue-and-wake
/// with bounded backpressure; never blocks the sender; never carries closures
/// (ADR-0037 §3 closed vocabulary).
#[derive(Clone, Debug)]
pub struct PlatformProxy { /* private: lane sender + owner ThreadId + wake */ }

impl PlatformProxy {
    /// Enqueue a window-open request. Never blocks — on any thread, including
    /// the owner (deferral replaces the old owner-side refusal; the blocking
    /// hazard lives in `PendingWindow::wait`, which is where it is refused).
    /// Fails fast with the rejected options returned so the producer can
    /// retry without rebuilding them (ADR-0027 §4 full-behavior contract).
    pub fn open_window(&self, options: WindowOptions)
        -> Result<PendingWindow, ProxySendError<WindowOptions>>;

    /// Coalesced, non-starvable quit flag — bypasses queue capacity
    /// (generalizes `ControlSender::request_quit`, control.rs:149-157).
    pub fn request_quit(&self);

    /// True iff the calling thread is the event-loop owner. Diagnostic only —
    /// correctness never depends on it (the types carry the guarantee).
    pub fn is_owner_thread(&self) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum ProxySendError<T: std::fmt::Debug> {
    #[error("platform owner lane is full (capacity {capacity})")]
    Full { capacity: usize, rejected: T },
    #[error("event-loop owner is gone")]
    OwnerGone { rejected: T },
}
```

**The reply is a claim slot, not a buffered send.** The current one-shot is a buffered `sync_channel(1)` (`control.rs:125`): the owner's send succeeds whenever the receiver handle is still alive, even if the reply is never read — so "unwind on reply-send failure" would miss every requester that drops its handle *after* the send, leaking the window. Instead, each request owns a small state machine, held jointly by the requester's `PendingWindow` and a bounded owner-side in-flight registry, with every transition under the slot's own private lock (SP-6: no lock or channel endpoint in any public signature):

```text
Pending   ──(owner completes creation)──▶  Delivered(result)
Pending   ──(requester drops handle)───▶  Abandoned { window: None }
Delivered ──(requester takes result)───▶  Claimed        // requester owns the window
Delivered ──(requester drops handle)───▶  Abandoned { window: Some(id) }
```

The slot transition is the at-most-once linearization point: a request is either `Claimed` (exactly one owner: the requester) or `Abandoned` (the event-loop owner unwinds — it closes and unregisters an already-created window, or skips creation entirely when it dequeues a command whose slot is already `Abandoned`). Abandonment wakes the owner; the owner sweeps its in-flight registry at each drain anchor and forgets `Claimed`/unwound entries, so the registry is bounded by lane capacity. A dying realm or finished worker that drops its `PendingWindow` therefore cannot leak a window **in any ordering** — this is strictly stronger than today's behavior, where a dropped requester is only logged (`platforms/winit/platform.rs:806-808`).

```rust
#[must_use = "dropping a PendingWindow disclaims the request; the owner skips or unwinds the window (§4)"]
pub struct PendingWindow { /* private: claim slot + owner ThreadId + wake */ }

impl PendingWindow {
    /// Blocking wait — worker threads only. On the owner thread this returns
    /// `WaitError::WouldBlockOwner` instead of deadlocking on the lane the
    /// caller itself drains (the winit taxonomy's `OwnerWouldBlock`,
    /// winit/platform.rs:112-118, promoted to the exact place blocking
    /// happens). Yields the full window handle, not a bare id — the same
    /// `Arc<dyn PlatformWindow>` today's cross-thread `open_window` resolves
    /// (`:964-969`; `PlatformWindow: Send + Sync`, traits/window.rs:92).
    pub fn wait(self) -> Result<Arc<dyn PlatformWindow>, WaitError>;
    /// Non-blocking poll; safe on any thread.
    pub fn try_take(&mut self) -> Option<Result<Arc<dyn PlatformWindow>, OpenWindowError>>;
}

#[derive(Debug, thiserror::Error)]
pub enum WaitError {
    #[error("waiting on the owner thread would deadlock the lane it drains; poll with try_take")]
    WouldBlockOwner(PendingWindow), // handle returned so the caller can still poll
    #[error(transparent)]
    Open(#[from] OpenWindowError),
}
```

The vocabulary starts at exactly the two verbs with real consumers today — `OpenWindow` (the existing lane command, `control.rs:21-26`) and `Quit` (the existing flag). Displays snapshots, clipboard verbs, prompt marshaling join **when a worker-side consumer exists**, per ADR-0034's no-plumbing-ahead-of-a-consumer discipline.

### 4. The audit's open questions, answered

- **Command ordering.** One bounded FIFO per owner; drained with a pre-read snapshot (`begin_drain`, `control.rs:177-183`) so a drain is finite and FIFO per batch (ADR-0027 §4). One producer's requests are observed in program order. `Quit` is deliberately *un*ordered: it is a flag riding any pending wake (`control.rs:149-157`, proven non-starvable when the queue is full by the existing lane test) and may overtake queued window requests — shutdown must not wait behind backpressure.
- **Re-entry and the drain-anchor contract.** On winit this holds **by construction today**: commands drain only from `user_event` at the top of the loop (`platforms/winit/platform.rs:774-776`), never from inside a nested callback. On AppKit and Win32 it does **not** hold by default — `CFRunLoopSource` fires and `PostMessageW` is dispatched inside nested modal run loops (`NSAlert`, modal dialogs, `DoDragDrop`) — so for those backends it is an *obligation*, not a property. The generalized lane therefore ships a `DrainGate`: an owner-thread re-entrancy latch that every drain must pass. The contract, which each slice-3 backend PR must demonstrate with a test: (a) a drain runs only when the gate is open; (b) the backend closes the gate for the duration of any nested/modal dispatch region and while the embedder's frame transaction is active; (c) a wake arriving while the gate is closed defers — the lane stays queued and the wake re-arms for the next top-level anchor. Under that contract a command handler that re-enters the OS pump leaves further commands queued, and the frame transaction stays uninterruptible (ADR-0027 §3 reentrancy gate). Owner-thread code cannot wait on the lane at all: `PendingWindow::wait` type-refuses the owner thread (§3).
- **Commands from destroyed nodes, cancellation, at-most-once.** All three collapse into the claim-slot protocol of §3: `Claimed` and `Abandoned` are mutually exclusive outcomes linearized at the slot, abandonment reaches the owner as a swept signal rather than a lost buffered send, and the owner converts it into effect-unwinding (close + unregister the orphan window) or effect-skipping (request still queued). Application of a claimed result into UI state is separately gated by the work-class freshness rules of ADR-0027 §6 (generational `ElementId`/`RenderId`, channel identity per realm incarnation).
- **Stale-frame results.** Lane replies are not frame-keyed and never commit mid-frame: results enter realm state only at Idle commit points (ADR-0027 §3), where the receiving work class applies its own freshness check — raster by `FrameEpoch` + `SurfaceGeneration`, assets by `ResourceGeneration`, realm lifetime by channel identity (ADR-0027 §6). A window opened for a realm that died meanwhile is reclaimed by the abandonment path: the dead realm's dropped `PendingWindow` marks the slot `Abandoned` and the owner unwinds.
- **Window identity.** `WindowId(u64)` (`traits/platform.rs:137`) stays the platform-internal native key and never becomes realm-facing identity: `AppRuntime` owns the only `WindowId → (RealmId, PresentationId)` map, and `PresentationId` is generational (`flui-foundation/src/id.rs:971`, `GenId` packing `:783-851`) — a recycled slot never compares equal (ADR-0037 §2). Within `flui-platform`, winit's ids are monotonic and never reused (`next_window_id`, `platforms/winit/platform.rs:225-229`); the macOS backend's pointer-as-id (`platforms/macos/platform.rs:164-165`) is a real ABA hazard named as a follow-up: it must move to a monotonic mint before that backend carries multi-window sessions.

### 5. `Clipboard` keeps `Send + Sync`; no affinity asserts on it anywhere

`Clipboard: Send + Sync` (`traits/platform.rs:473`) is unchanged, and this ADR takes one consistent position: **clipboard resolution and the plain-text operations are thread-safe and get no owner asserts** — not in slice 1, not later. The X11/arboard implementation is genuinely thread-safe, and `NSPasteboard` is a process-wide singleton documented safe for off-main reads (ADR-0034, "What is deferred"). Reversing ADR-0034 (which deliberately stashes `Arc<dyn Clipboard>` in `AppBinding`, `binding.rs:110`, resolved pre-`run()` on Android/web where no later resolution point exists, `runner.rs:2290-2297`) would churn a just-landed seam for no safety gain. This **supersedes ADR-0034's suggestion** of a future `debug_assert!` inside `MacOSClipboard` methods: asserting main-thread affinity on operations the same paragraph documents as off-main-safe is a contradiction, and a worker legitimately reading the sanctioned `AppBinding` clipboard slot must not panic in debug. Where pasteboard affinity is real — rich clipboard *items*, promised/lazy pasteboard data — the obligation lands structurally when the data-transfer design (audit U6/U18, designed separately) ships those operations: either on `OwnerPlatform`, where the `!Send` type carries the guarantee, or inside a marshaling `Clipboard` implementation that crosses to the owner via the lane internally rather than pushing the obligation onto callers. This ADR only fixes where the *calls* may run; `write_to_clipboard`/`read_from_clipboard` stay untouched on `Platform` (§2) and their fate — re-home, replace, or delete — is that design's decision.

### 6. Realm composition (ADR-0027/0037)

The capability *is* the platform-facing half of realm affinity, not a competitor to it:

- **The host slot is loop-scoped, not realm-scoped.** `OwnerPlatform` lives in a new owner-thread TLS slot in `flui-app` (`OWNER_PLATFORM_HOST`), *separate from* the realm host (`PLATFORM_REALM_HOST`, `runner.rs:104`): installed at `on_ready` entry, cleared after `Platform::run` returns on backends where it returns (winit, headless, Android); on macOS `run` never returns (`terminate:` exits the process), so clearing is moot. It is deliberately **not** cleared in `teardown_platform_realm` (`runner.rs:2201-2202`) — a realm's teardown must not strand the loop's capability, because the loop may host another realm before it exits (hot-restart does exactly this today, `install_platform_realm`, `runner.rs:817-840`). It cannot live in `AppBinding` fields without stripping `AppBinding`'s auto-traits.
- **Multi-realm honesty.** ADR-0027 §1 makes realm count per owner thread embedder policy, and the loop-scoped slot is *compatible* with several realms sharing one loop's capability — but today's realm host is a single-realm slot, so multi-realm hosting on one thread remains **follow-up design work**, not something this ADR claims to deliver. What this ADR fixes is the scoping bug that would have made it impossible: a capability torn down with the first realm.
- **The accessor is guarded, not ambient.** Build/layout/paint run on the owner thread, so a TLS capability slot is exactly the kind of ambient authority trigger #22 exists to fence. Three fences, all landing in slice 2: (a) the accessor (`owner_platform()`) is `pub(crate)` to `flui-app`'s app module — it is a composition-root affordance, not framework API, and is never re-exported; (b) `owner_platform` joins the `capabilities` token list in `scripts/check-frame-capability-scope.sh` **in the same PR**, so the trigger-#22 brace scanner mechanically rejects any acquisition inside `build`/`perform_layout`/`paint`/composite bodies across all crates; (c) the accessor `debug_assert!`s the scheduler is not inside a frame phase, so even reflection-style indirection trips in debug. Because post-bootstrap `open_window` is deferred (§1), a violation that slips all three fences still cannot mutate the window map mid-frame — it can only enqueue.
- Because `dispatch_platform_realm` rejects wrong threads (`runner.rs:847-850`), every realm callback already executes where the capability lives; realm code uses it without any check — the `!Send` bound makes "checked at compile time" literal.
- **No `BuildContext` capability in this ADR** (same restraint as ADR-0027 §9). If a widget-tree consumer later needs platform access, it lands as a handle acquired in `init_state`/`did_change_dependencies` per trigger #22, with its own token in the scanner.

### 7. House-constraint compliance

- **SP-6:** no `MutexGuard` or channel endpoint appears in any public signature above; the claim slot and lane sender are private fields of `PendingWindow`/`PlatformProxy`.
- **Sync pipeline:** nothing here is `async fn`; every proxy method is a non-blocking send; the only blocking call (`PendingWindow::wait`) is worker-only by typed refusal. `flui_platform::Task` appears nowhere in this design — the prompt methods that return it stay where they are rather than being promoted onto the new capability (§2) — and `TaskToken` remains the framework's one async mechanism.
- **FR-036:** port-check trigger #9 scans `flui-view/-foundation/-tree/-engine/-rendering/-interaction` (`scripts/port-check.sh:1092-1096`) — `flui-platform` is outside the scanned set, so no marker is mechanically required; the new `dyn` boundaries (`Arc<dyn Platform>` inside `OwnerPlatform`, slice-3 `Arc<dyn OwnerOps>`) are nonetheless recorded here as their sanctioning rationale should the registry scope widen.
- **Panic policy:** affinity violations in slice 1 are `debug_assert!` (a violated OS-affinity invariant is a bug, not an environment failure); public methods keep typed `Result`s for liveness errors.

## Migration — incremental slices

### Slice 1 (first PR): runtime backstop, with its testable core where CI runs tests

The compile-time capability cannot land first — it requires the `PlatformReadyCallback` signature break. What lands first is the missing runtime floor, sized so the later slices only *strengthen* it:

1. **`OwnerAffinity` lands in `flui-foundation`** (Layer 1; `flui-platform` at Layer 5 gains a layer-legal dependency on it — its only workspace-internal dependency today is `flui-types`):
   ```rust
   // crates/flui-foundation/src/affinity.rs
   pub struct OwnerAffinity { owner: std::sync::OnceLock<std::thread::ThreadId> }
   impl OwnerAffinity {
       pub const fn new() -> Self;
       /// Bind at `Platform::run` entry (and `MacOSPlatform::with_config`,
       /// which already documents "must run on the main thread", macos/platform.rs:83-85).
       pub fn bind_current(&self);
       /// `debug_assert!` the caller is the bound owner; traces op name on violation.
       pub fn debug_assert_owner(&self, op: &'static str);
   }
   ```
   Its unit tests (bind, cross-thread violation caught, idempotent re-bind rejection) live in `flui-foundation`, whose suite **runs in the normal CI test job** — this is the slice's CI-verified deliverable.
2. macOS: `bind_current` in `with_config` + `run`; `debug_assert_owner` at entry of `open_window`, `quit`, `active_window`, `displays`, `primary_display`, plus a direct `msg_send![class!(NSThread), isMainThread]` debug check — on AppKit the owner thread must *be* the OS main thread, which `ThreadId` equality alone cannot express. **No assert on `clipboard()` or inside `MacOSClipboard`** (§5 — those operations are documented off-main-safe; asserting them would contradict this ADR's own contract and panic legitimate ADR-0034 callers).
3. Windows: same recording in `run`; asserts on `open_window`/`quit`/`active_window`.
4. winit/headless: no asserts needed — winit already returns typed state errors keyed by `owner_thread` (`platforms/winit/platform.rs:106-123`); headless is single-threaded by construction.

**Honest scope statement (Definition of Done):** CI proves *compilation only* for everything in items 2–3: the macOS/Windows backends are exercised solely by cross-typecheck (which runs nothing), **and** `flui-platform`'s own test suite is excluded from the CI gate (STATUS_HEAP_CORRUPTION investigation) — so no assert wiring in that crate is executed by CI either. That is exactly why the primitive itself lives in `flui-foundation`: its behavior is CI-verified even though its backend wiring is not. Backend wiring is verified locally (`just test-crate flui-platform` on the respective OS); re-including even a targeted subset of `flui-platform` tests in CI is blocked on the STATUS_HEAP_CORRUPTION investigation and stays out of this ADR's scope.

### Slice 2: `OwnerPlatform` + callback flip — every backend, every embedder, one PR

`OwnerPlatform`, `PlatformProxy`, and the claim-slot reply protocol land wrapping the *unchanged* `Platform` trait (each backend converts to `Arc<Self>` at `run` entry, as winit already does — `winit/platform.rs:877`). Flipping `PlatformReadyCallback` breaks `run` in **all six backends in the same PR** — winit, macOS (`macos/platform.rs:122`), Windows (`windows/platform.rs:896`), web (`web/platform.rs:124`), Android (`android/mod.rs:172`), headless (`headless/platform.rs:97`) — because cross-typecheck forces the Win32/AppKit edits to compile immediately; this is the deliberate cost of a single-signature seam. Also in this slice:

- **Hoist winit handler dispatch out of `with_state`** — take or snapshot the registered handler outside the lock, then invoke it (`:558-563`, `:598-603`, `:614-620`, `:632-645`, `:650-657`). Precondition for any owner-thread platform call from handler context not deadlocking on the non-reentrant state mutex, and a latent-bug fix in its own right.
- The claim-slot protocol replaces the buffered one-shot in the winit lane, with lane tests proving: dropped-before-completion skips creation; dropped-after-delivery unwinds the window (platform state ends windowless); claimed-then-dropped keeps it.
- The owner-TLS `OWNER_PLATFORM_HOST` slot (install at `on_ready`, clear after `run` returns) and its three accessor fences, including the `owner_platform` token in `check-frame-capability-scope.sh` (§6).
- **Pre-run flows migrate into `on_ready`.** Three of the four are behavior-preserving because their backends invoke `on_ready` synchronously on the owner thread before the loop pumps: the Win32 examples (`windows11_demo.rs:54` et al.; `WindowsPlatform::run` calls `on_ready` before the message loop, `windows/platform.rs:896-903`), the web bootstrap (`runner.rs:2620-2624`; `WebPlatform::run` calls `on_ready` before starting the RAF loop, `web/platform.rs:123-133`), and `run_direct` (`direct.rs:101` — the reorder its own comment already names as the known fix). The fourth, `run_android`'s pre-run bootstrap (`runner.rs:2305-2307`: window, GPU init, realm mount), moves inside `on_ready`, which Android delivers at the first `Resume` — the backend's own module doc names `Resumed → on_ready() → create surface` as the intended sequence (`android/mod.rs:13`); the pre-run call predates it. Android is the one migration with behavioral risk (GPU init shifts from before the loop to first `Resume`) and **must be validated on-device before slice 3 may delete the trait method**.
- The remaining first-party `platform.open_window(...)` call sites already inside `on_ready` (`bootstrap_desktop`, `runner.rs:1809`; the winit examples) mechanically migrate to `owner.open_window(opts)?.expect_ready()?` — `Ready` is guaranteed there. The run-less headless test sites (`binding.rs:3997`, `ui_realm.rs:748`, `runner.rs:2965-2977`) stay on the unchanged trait method until slice 3 gives them a mint (below).

### Slice 3: trait surgery + lane generalization

Gated on the slice-2 Android on-device validation. The §1 method list leaves `Platform`; the five dead stubs are deleted (§2); `OwnerPlatform` re-targets `Arc<dyn OwnerOps>` (`pub(crate)` object-safe trait carrying the moved methods, typed errors replacing their `anyhow` signatures); `control.rs` moves to `shared/owner_lane.rs` parameterized by wake hook + `DrainGate`; macOS/Windows adopt the lane for their proxy verbs, each in its own PR carrying the ADR-0027 §3 wake contract **and a drain-anchor gate test** (§4: a wake delivered during a nested modal run loop must defer, not drain). Android/web/headless implement `OwnerOps` trivially (single-threaded loops). The run-less headless test flows get a concrete-type mint in the same PR that removes the trait method: `HeadlessPlatform::owner_platform(&self) -> OwnerPlatform`, binding the calling thread as owner — sound because possession proves the thread and headless never starts a loop; the three test call sites migrate to it.

### Slice 4+ (per-consumer): vocabulary growth

Proxy verbs beyond `OpenWindow`/`Quit`; macOS monotonic `WindowId` mint; the prompt-consolidation follow-up ADR (§2); deleted stubs returning with a first real implementation and consumer — each gated on that consumer.

## Alternatives considered and rejected

- **A. Runtime checks only (asserts everywhere, no capability type).** Keeps every one of ~30 methods a latent wrong-thread call that only a debug build on the affected OS can catch — and the two affected backends are exactly the ones CI never executes (cross-typecheck compiles, runs nothing). The type-level design turns the whole class into compile errors on every host. Slice 1 ships the asserts anyway, but as scaffolding under the capability, not as the end state.
- **B. Lifetime-bound `ActivePlatformContext<'event_loop>` (the audit's sketch).** A borrow handed to `on_ready` dies when `on_ready` returns — but most owner-thread platform work happens *after* bootstrap, inside later platform callbacks (realm dispatch, window handlers). An owned `!Send` value with runtime liveness errors serves both; a lifetime-bound context would force re-minting machinery per callback for no added safety (`!Send` already pins the thread).
- **C. Rebuild `foreground_executor` and marshal closures to the main thread (GPUI shape).** The method was already removed, and ADR-0027 §9 makes the generic run-a-closure primitive a standing rejection: closures erase *what* crosses the boundary, defeating bounded typed backpressure, freshness gating, and the closed-vocabulary audit trail (ADR-0037 §3 explicitly bans `Box<dyn FnOnce()>` payloads).
- **D. Make `Platform` itself `!Send` (whole-trait affinity).** Over-rotates: `background_executor()` acquisition, `on_*` callback registration, cross-thread `quit()` (the runner's error paths call it, `runner.rs:1814-1869`), and ADR-0034's pre-`run()` clipboard resolution on Android/web (`runner.rs:2290-2297`) are legitimately cross-thread-safe, and a `!Send` `Box<dyn Platform>` would forbid all of them. Splitting the surface keeps each half honest instead of laundering both through one wrong bound in either direction. (Note this rejection no longer leans on pre-run `open_window` — slice 2 migrates those flows into `on_ready`, so they cut against neither D nor the chosen design.)
- **E. Transparent internal marshaling (every `&self` method silently enqueues-and-blocks when off-owner).** Hides a blocking rendezvous inside innocuous-looking calls; deadlocks when the owner itself calls (the exact trap winit's `OwnerWouldBlock` taxonomy exists to refuse, `platforms/winit/platform.rs:112-118`); and inverts ADR-0027 §4 — backpressure must be a typed, visible `Full{rejected}` at the producer, not an invisible stall.
- **F. Message-only for everything, including reads and bootstrap (pure actor).** Bootstrap *must* be direct — the deadlock that forced winit's `ACTIVE_EVENT_LOOP` fast path (module doc `:18-30`) proves the lane structurally cannot serve `on_ready` — and owner-thread reads (`displays`, `active_window`, `window_appearance`) would pay queue+wake+reply for data sitting one call away. The chosen design is the honest hybrid: direct for bootstrap and reads, deferred through the lane for post-bootstrap window creation, marshaled for workers.
- **G. Direct owner-thread creation everywhere via TLS widening (this ADR's own first draft).** Widening the `ACTIVE_EVENT_LOOP` publication from `on_ready` to every `ApplicationHandler` callback lets `OwnerPlatform::open_window` create directly from any owner-thread context — and is rejected because that context is not uniform: winit invokes `on_window_event` handlers *while holding its non-reentrant state mutex* (`with_state` closures, `:558-563` et al.), so a handler's direct creation re-enters `with_state` (`:401`) and deadlocks silently (`parking_lot` blocks, no panic); and calls from realm/frame code dispatched outside the lock (`win.callbacks()` paths, `:610`, `:668`) would *succeed* mid-frame-transaction, mutating the platform window map where today they fail with a typed `OwnerWouldBlock` (`:938-940`). Trading a typed error for a silent deadlock plus an unguarded mid-frame mutation is strictly worse than deferral, which serves the same callers with neither hazard.

## Consequences

**Positive.** Wrong-thread platform calls become unrepresentable for capability holders on all backends, including the two CI cannot execute; owner-thread window creation after bootstrap becomes possible on winit for the first time (today it errors `OwnerWouldBlock`) without opening a blocking or re-entrant path — deferral serves it safely; the winit lane's semantics (bounded, typed, rejected-payload-returning, admission-gated) get promoted from backend-internal detail to the cross-thread contract, strengthened by the claim-slot protocol from "dropped requester is logged" (`:806-808`) to at-most-once with guaranteed unwind; `MacOSPlatform`'s `unsafe impl Send/Sync` "by convention" (`macos/platform.rs:46-53`) becomes deletable in slice 3 (the affine surface leaves the shared trait, shrinking the unsafe justification to the residual methods); five dead stub methods leave the public API; the audit's open protocol questions get pinned answers whose winit-specific parts are backed by tested code and whose AppKit/Win32 parts are named per-backend obligations with test requirements.

**Costs.** `PlatformReadyCallback` is a breaking signature change that must touch all six backends and every embedder/example in one PR (pre-1.0, sanctioned by ADR-0027 §9's break-acknowledgement posture); slice 3 is a second, distinct break class — it removes 13 methods from the public `Platform` trait (8 moved, 5 deleted), breaking any external `Platform` implementor, not just callers; the lane generalization moves `control.rs` (≈500 lines incl. its tests) into shared code that macOS/Windows must wire wakes and drain gates for; the claim-slot protocol is a genuinely richer state machine than a buffered one-shot and carries its own test matrix; two platform-shaped types (`OwnerPlatform`, `PlatformProxy`) join the public API and must document their thread-affinity contracts per ADR-0027 §9.

**Risks.** The Android bootstrap migration shifts GPU init from before the loop to the first `Resume` — behaviorally the backend's documented intent, but unvalidated until it runs on-device; slice 3 is explicitly gated on that validation. The winit handler-dispatch hoist changes what handlers may observe (they run after, not inside, the state update) — existing handler code must be re-audited in that PR. Post-bootstrap owner-thread `open_window` on winit resolves a turn later than the request (`Pending`), which callers must design for; bootstrap callers are unaffected (`Ready` guaranteed in `on_ready`). The orphan-window unwind changes observable behavior for a requester that drops its handle mid-flight (window may flicker open/closed instead of leaking); strictly better, but a release-notes item.

## Follow-ups

- **Multi-realm hosting on one owner thread** — the loop-scoped `OWNER_PLATFORM_HOST` removes this ADR's obstacle, but the realm host itself is single-realm today (`runner.rs:104`); hosting several realms per loop is its own design (ADR-0027 §1 policy surface).
- **Prompt consolidation** — its own follow-up ADR: today's Windows-only implementation runs on a dedicated STA thread and returns `flui_platform::Task` (drop does not cancel, `task.rs:107-110`); the consolidation to the framework's one async mechanism (claim-slot pending reply vs. `TaskToken`) and the macOS main-thread-panel affinity story land together there. This ADR leaves the methods untouched and keeps them off `OwnerPlatform` (§2).
- **macOS `WindowId` mint** — replace pointer-as-id (`macos/platform.rs:164-165`) with a monotonic counter before multi-window sessions on that backend (ABA via pointer reuse).
- **Data-transfer alignment (U6/U18)** — the clipboard/DnD ownership design consumes §5's contract (structural affinity via `OwnerPlatform`, or a marshaling implementation) and decides rich-item transport; if it re-homes clipboard resolution onto `OwnerPlatform` entirely, ADR-0034's `AppBinding` slot becomes a realm-scoped read model of it.
- **`BuildContext`-level platform capability** — only with a concrete widget consumer; token joins `check-frame-capability-scope.sh` in the same change (trigger #22).
- **Win32 message-only-HWND wake + AppKit `CFRunLoopSource` wake** — the slice-3 lane adoptions; each is its own PR carrying the ADR-0027 §3 wake contract and the §4 drain-gate test.
- **flui-platform CI test re-inclusion** — the honest gap in slice 1's verification story closes only when the STATUS_HEAP_CORRUPTION investigation lands; tracked there, not here.
- **wasm posture** — single-threaded: `OwnerPlatform` is trivially constructible by the RAF loop; `PlatformProxy` is inert (`OwnerGone` after teardown never occurs because teardown never runs — ADR-0027 §7 notes web keeps its owner host for the page lifetime). No special casing required, but the web backend's `OwnerOps` impl should assert nothing rather than emulate marshaling.
