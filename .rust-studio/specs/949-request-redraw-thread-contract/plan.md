# Plan — issue #949: owner-route `MacOSWindow::request_redraw`'s AppKit body

Status: Approved (user, "proceed when the auditor lands", 2026-09-16). Phase 4 build
COMPLETE (mechanism proven + always-run red-by-revert verified by the orchestrator).
Phase 5 review complete. Supersedes `plan-v1-rejected-docs-only-superseded.md`, whose
docs-only direction is recorded in that file for its history and was deliberately NOT
taken: it left the unsound cross-thread call in place, which the machine this plan ran on
(real AppKit link + run) can actually fix.

## 1. Problem and resolution

`MacOSWindow::request_redraw` messages AppKit (`contentView`, `setNeedsDisplay:`) directly,
from whatever thread completed a future: the scheduler's `on_frame_scheduled` hook installs
`FrameWakeHandle::wake_frame` as the producer, and an `AsyncDriver` task waker fires on the
thread that completed the future. AppKit's main-thread convention makes that a contract
violation, and the one backend CI compiles (AppKit, via `cross-typecheck`) it never links or
executes — so nothing goes red. The relay ADR-0045 decision 5 mandates (a channel pushed by
the raster side, drained by the platform's event-loop waker on the owner thread) is the real
end-to-end fix and is scoped to #559/#551; it needs `PlatformProxy` transports every lane-less
backend currently lacks, so it is not guessed at here.

**Resolution (option 2, mirroring issue #1124's clipboard owner-route):** route
`request_redraw`'s AppKit body onto the window's **owner lane** — the AppKit main queue for
production windows, a caller-supplied serial queue for test windows — so the messages always
execute under the lane guard and never bare on the caller. This makes `request_redraw` the
macOS backend's ONE mechanically-enforced thread-affine site; it does NOT make the async lane
safe end-to-end (the wake hook still calls `request_redraw` directly; only the method's AppKit
body is routed), which the runtime-contract correction states explicitly.

Market corroboration (Prime Directive rule 2): Zed GPUI and the gpui-ce fork route every
AppKit-touching window operation through `DispatchQueue::main().exec_async_f(...)` or a
main-thread `ForegroundExecutor`, keeping window state in `Arc<Mutex<MacWindowState>>`. Their
`exec_async` is sound only because upstream stashes window state in an ivar Arc with
`setReleasedWhenClosed:NO` retention — machinery FLUI does not have. FLUI's synchronous
`exec_on_owner` with the borrow-pinned `&self` capture is the citable, justified divergence.

## 2. Design (final)

### 2.1 New shared module `crates/flui-platform/src/platforms/macos/owner_lane.rs`
Extract the clipboard's lane machinery (issue #1124) so both clipboard and window route
through one implementation:
- `owner_queue()` — `&'static dispatch::Queue` = the AppKit main queue, kept in a `OnceLock`
  (identity of the cached queue is load-bearing: the reentrancy marker compares ADDRESSES).
- `#[cfg(test)] test_owner_queue()` — one process-wide serial dispatch queue for all test
  instances (`cargo test` never pumps the AppKit main thread, so the main queue would never
  drain and `exec_sync` would block forever).
- Reentrancy marker: `thread_local! { static ON_OWNER_QUEUE: Cell<Option<*const Queue>> }`,
  set by the RAII `OnOwnerQueueGuard` (whose `Drop` clears only if the marker still equals
  this guard's own owner, so a nested block on another lane is cleared by ITS guard).
  `on_owner_queue(owner)` probes by address identity.
- `exec_on_owner<R: Send>(owner, owner_is_main, f: FnOnce() -> R + Send) -> R` — runs `f`
  directly when already on the lane or (`owner_is_main` && `isMainThread`); otherwise
  `owner.exec_sync` with the guard set, the body wrapped in
  `catch_unwind(AssertUnwindSafe(f))`, and `resume_unwind` replaying the payload on the
  caller (the dispatched body crosses an `extern "C"` trampoline with no panic catch, so an
  unwinding panic through libdispatch's C frames would be UB).
- `f` is `Send`-bound; a caller that captures a raw `*mut Object` (`id` is `!Send`) is
  rejected at compile time — the clipboard and window closures therefore capture only `&self`
  and plain data.

Hazards documented in the module: **un-drained lane stall** (a worker calling onto a main-lane
owner before `Platform::run` starts `[NSApp run]` blocks until it does; `exec_sync` has no
timeout) and **cross-lane reentrancy deadlock** (a block on lane A dispatching to lane B and
back deadlocks; the probe rescues only same-lane nesting, `is_main` only main-lane instances).

### 2.2 `window.rs`
- `MacOSWindow` gains `owner: &'static dispatch::Queue` + `owner_is_main: bool`; `Clone`
  copies both (they are properties of the window, not of any one handle); `Debug` skips them
  (`dispatch::Queue` has no useful Debug form).
- `new(...)` → private `new_inner(..., owner, owner_is_main)`; `new()` passes the main lane
  and `true`; `#[cfg(test)] for_test(owner)` constructs **on the lane** via `exec_on_owner`
  (AppKit window construction is thread-affine) with `owner_is_main = false`.
- `request_redraw` rewritten:
  ```rust
  let owner = self.owner;
  let owner_is_main = self.owner_is_main;
  super::owner_lane::exec_on_owner(owner, owner_is_main, || unsafe {
      let content_view: id = msg_send![self.ns_window, contentView];
      if content_view != nil { let _: () = msg_send![content_view, setNeedsDisplay: YES]; }
  })
  ```
  The closure captures `&self`, NOT the raw id — `&MacOSWindow: Send` via the `unsafe impl
  Sync`, so sending the id across the `Send` boundary is impossible. The SAFETY comment states
  the honest guarantees: this is the ONE mechanically-routed AppKit message site; every other
  site is AppKit-callback-delivered, `debug_assert_appkit_main_thread`-guarded (ADR-0039), or
  flui-app-confined — call-graph facts, not enforcement; `Drop` is a deliberate exception (the
  scheduler can release its last wake-frame clone on an IO lane), and its off-main a11y
  `bridge.shutdown()` is a TRACKED RESIDUAL owned by issue #1194. The `unsafe impl Send`/`Sync`
  lines stay byte-identical (load-bearing conformance evidence).
- `#[cfg(test)] redraw_thread_probe` — two-cell TLS split: `record_routed_block_thread` (set at
  the top of the routed lane block) + `record_msg_send_thread` (set at the message send) into a
  single-writer `SINK: Mutex<Option<(Option<ThreadId>, ThreadId)>>`. `record_msg_send_thread`
  reads the separate block-top cell, so the routed witness is never self-clobbered.
- A `#[ignore]`d window integration test `request_redraw_is_owner_routed`: constructs a real
  NSWindow on the TEST lane, then (a) an on-lane nested `request_redraw` must complete inline
  with no deadlock, (b)+(c) an off-lane worker `request_redraw` must complete crash-free AND
  record a routed block-top thread (an un-routed call never records it — the routing
  assertion), with `msg_send_thread == routed_block_thread`. Deliberately NOT asserting thread
  identity differs from the caller: modern libdispatch runs an uncontended serial-lane
  `dispatch_sync` INLINE on the calling thread, so LANE MEMBERSHIP, not thread identity, is the
  contract. Ignored with an honest reason: a bare `cargo test` process has no NSApplication
  connection and NSWindow construction SIGABRTs (observed on this machine through
  `_CFBundleGetValueForInfoKey`).

### 2.3 `clipboard.rs` (behavior-neutral extraction)
Delete the local `owner_queue`/`test_owner_queue`/`OnOwnerQueueGuard`/`ON_OWNER_QUEUE`;
`with_pasteboard_on_owner` delegates to `super::owner_lane::exec_on_owner(self.owner,
self.owner_is_main, || ...)` with the pasteboard still re-resolved per-op inside the block. All
5 clipboard tests unchanged — the regression guard for the extraction.

### 2.4 `docs/runtime-contract.toml`
`raster-wake-relay-precedes-thread-spawn` gains `CORRECTION (2026-09-16, issue #949)`: the
method's AppKit body is owner-routed via the shared machinery, so the async lane no longer
falsifies the `unsafe impl Send` SAFETY precondition on its own; explicitly NOT claiming
end-to-end safety (wake hook still calls `request_redraw` directly). `state = "partial"`
(`owner_issue = 559`, `mechanical = false`) is retained — the relay itself remains the open
requirement. The two load-bearing evidence literals are byte-untouched: `unsafe impl Send for
MacOSWindow {}` (window.rs) and `pub fn run_until_shutdown(mut self) {` (flui-engine).

## 3. Test strategy

- **Always-run AppKit-free routing carrier** in `owner_lane.rs`: (a) `exec_on_owner` runs
  inline when already on the lane (reentrancy), (b) an off-lane spawn routed through
  `exec_on_owner` observes the on-lane marker. Red-by-revert VERIFIED by the orchestrator
  (2026-09-16): removing the guard+dispatch from `exec_on_owner` fails (b) exactly on the
  marker assertion while (a) still passes. These run in every `cargo test -p flui-platform`
  on every platform.
- The window integration test is the site-level carrier and is `#[ignore]`d when AppKit is
  inaccessible from a bare test process; its routing assertion is None-capable (un-routed calls
  never record a block-top thread).
- Clipboard tests unchanged (extraction regression guard).

## 4. Environment

`crates/flui-platform` links AppKit locally, so builds need the lld-strip wrapper shim
(`--config 'build.rustc-wrapper="/tmp/lldstrip-wrapper.sh"'`): the committed `.cargo/config.toml`
appends `-fuse-ld=lld` for Apple targets, which this machine's clang 17 rejects. `.cargo/`
stays untouched (user constraint). `just runtime-conformance-check` needs the `/tmp/py312shim`
python3 symlink (system python3 is 3.9, no tomllib); `just ci` needs `/tmp/bashshim` for
port-check (bash 3.2 has no mapfile).

## 5. Verification (expected at build close)

- `cargo test -p flui-platform` — 161 passed / 1 ignored (the AppKit window test) / 0 failed;
  headless suite 12 passed; `platform_it` 34 passed / 33 failed / 8 ignored is PRE-EXISTING on a
  live Mac (macOS-live `MacOSPlatform` trips an ADR-0039 debug assert under libtest; confirmed
  identical on a stashed clean tree; CI runs that suite headless on Linux).
- `cargo clippy -p flui-platform --all-targets -- -D warnings` exit 0; `cargo fmt --check` clean;
  `cargo check` exit 0.
- `just runtime-conformance-check` exit 0, 56 contracts; evidence literals byte-untouched.
- Honest disclosure: site-level (real-NSWindow) RED/GREEN evidence is NOT executable in a bare
  `cargo test` on this machine (AppKit SIGABRT on construction); the always-run machinery test
  is the executable carrier and its red-by-revert is verified.

## 6. Explicitly NOT in this change

- The `PlatformProxy` redraw verb / the ADR-0045 D5 relay — #559/#551 (needs lane-less-backend
  transports).
- The other ~45 AppKit message sites reachable off the owner lane: swept and filed as #1194,
  with `Drop`'s off-main a11y shutdown as that issue's named owner.
- Windows/Linux/Android backends (identical trait-level rule, different mechanisms).
- Per-method thread-rule prose on all 48 `PlatformWindow` methods — a separate docs concern
  (the earlier docs-only draft moved aside as `plan-v1-…`).

## 7. Risk

Same-lane reentrancy, cross-lane deadlock, un-drained-lane stall, and OnceLock-identity
dependence are documented in `owner_lane.rs` and covered by the always-run tests. The main
residual is that an off-lane AppKit call in production would be caught only by a real-NSWindow
test on a run-loop-pumping process — which this plan's opt-in integration test provides and CI
will not run until the macOS backend is ever linked in CI.
