# Plan — issue #1194: sweep the macOS window surface onto the owner lane

Status: Phase 2.5 complete — all three plan-review verdicts (harsh-critic + unsafe-auditor +
concurrency-specialist) returned RESHAPE NEEDED; the bounded-wait teardown they attacked was already
replaced by the fire-and-forget design, and every surviving finding is folded into §1-§5 below.
Awaiting Phase 3 approval; build starts only after the gate.

## 1. Problem and resolution

`request_redraw` is owner-routed (#949): its AppKit messages execute on the window's owner lane
(AppKit main queue in production, one process-wide serial queue for tests) via the shared
`owner_lane` machinery (#1124→#949). Every OTHER AppKit message site in the macOS backend is
reached only by call-graph topology — AppKit main-thread delivery (delegate/view/event callbacks),
**debug-only** `debug_assert_appkit_main_thread` / `OwnerAffinity::debug_assert_owner` guards
(both compiled out of release), or flui-app embedder confinement to the owner thread. None is
mechanically enforced, and `MacOSWindow` is `Send + Sync` by `unsafe impl`, so an off-owner call
on any of them is reachable, silent, and UB by Apple's own doctrine where it bites. The #949-era
sweep filed this as **#1194**, with `Drop`'s off-main a11y `bridge.shutdown()` as the named owner.

A fresh scout inventory (2026-09-16) counts **177 ObjC message sites** in `platforms/macos/`
across 7 files, classified by thread reach: 93 embedder-confined (the public window surface —
PlatformWindow / WindowTrait / MacOSWindowExt / raw-window-handle — plus unguarded `app_path`),
37 debug-assert-guarded (platform-level entries + ctor chain + display enumeration), 27
AppKit-delivered (delegate/view/event callbacks), 19 already-routed (request_redraw, the
exec_on_owner probe, clipboard), 1 off-owner-reachable (`msg_send![release]` in `Drop`) plus the
feature-gated non-message a11y `bridge.shutdown()` residual.

**Resolution:** route the whole public window surface onto the owner lane using the same direct-path
probe as `request_redraw` — one private helper `route_on_owner` that every swept method body
travels through — so the lane, not a debug assert, is the enforcement. Market corroboration
(Prime Directive rule 2): GPUI/gpui-ce route every AppKit-touching window operation through
`DispatchQueue::main().exec_async_f(...)` or a main-thread `ForegroundExecutor` with
`Arc<Mutex<MacWindowState>>`; their async marshaling is borrow-free only because window state is
ivars-held with `setReleasedWhenClosed:NO` retention, an affordance FLUI lacks — FLUI's
synchronous `exec_on_owner` with a borrow-pinned `&self` capture is the citable, justified
divergence (already established in #949). Apple's own AppKit Thread Safety Summary classes NSView
"Main Thread Only" and NSWindow "generally not thread-safe" (one-thread-at-a-time); the only
documented off-main allowances are `retain`/`release` machinery, `[NSThread isMainThread]`, and
window **creation**; `setNeedsDisplay:` is explicitly NOT documented thread-safe (the Summary lists
it among methods a secondary thread "must not" call to trigger redraw) — so routing every
AppKit-messaging window body is the conservative-correct reading, and `request_redraw`'s #949
routing is corroborated, not folklore.

## 2. Design (final)

### 2.1 Site-class disposition (the sweep map)

| Class | Sites | Handling |
|---|---|---|
| A — public window surface, bodies that message AppKit | 36 bodies (PlatformWindow 12 + `request_redraw` + WindowTrait 12 + MacOSWindowExt 11) | **route** via `route_on_owner` |
| — public window surface, bodies that are pure-Rust reads | position/size/scale-factor-style reads of the `state` mutex | **not routed** (no AppKit contact; routing a pure-Rust read = a blocking rendezvous that buys nothing) |
| C — platform-level entries | `with_config`, `run`, `quit`, `open_window`, `active_window`, `displays`, `primary_display`. `app_path` is **untouched** — the planned assert was folded out: its body is `NSBundle` singleton reads (thread-safe), the method is a one-shot path with no reachability signal, and asserting it is scope creep with regression risk | **keep debug assert + document** (ADR-0039 slice 1 decided these; `quit` is NOT documented cross-thread reachable — `OwnerPlatform` is `!Send`, so the platform object is owner-confined and `terminate:` stays off the routed surface; the debug assert is the slice-1 interim backstop, and the `!Send` `OwnerPlatform` capability migration is slice 3's record) |
| B — AppKit-delivered callbacks | FLUIWindowDelegate + FLUIContentView + `convert_ns_event` family (27) | **document-only** (AppKit delivers on main by construction; routing would be a guaranteed inline no-op on main-lane owners and a needless cross-lane dispatch hazard on test-lane instances) |
| E — raw-window-handle accessors | `window_handle`, `raw_window_handle`, `display_handle` | **debug assert + document** (the `contentView` getter is a pure accessor; `RawWindowHandle` is `!Send` so it cannot cross `exec_on_owner<R: Send>`; and the enforcement IS genuine and upstream — verified against the pinned crates: raw-window-handle 0.6.2's `AppKitWindowHandle` has plain `new` with main-thread-only documented, while raw-window-metal **1.1.0** `Layer::from_ns_view` (lib.rs:403) runs `MainThreadMarker::new().expect("can only access NSView on the main thread")` — a runtime HARD panic before surface creation. The residual this class leaves routed-out is the single un-routed `contentView` getter message itself, a read-only accessor) |
| D — `Drop` | the AppKit tail: a11y `bridge.shutdown()` + `msg_send![release]` | **route + dead-lane escape** (see 2.4) |

### 2.2 New routing helper `route_on_owner` (window.rs)

The one new private helper is the throat every swept class-A body travels through, and the seat of
the test probe. It is a free function (not a method) so the always-run AppKit-free test can exercise
it on the shared test lane without constructing a real window. The probe key is derived from
`#[track_caller]`'s `(file, line)` under `cfg(test)` — production call sites carry NO probe strings
(the rejected alternative — passing a `&'static str` key through every production signature — was
stringly-typed; a maintainer concern).

```rust
#[cfg_attr(test, track_caller)]
pub(super) fn route_on_owner<R: Send>(
    owner: &'static dispatch::Queue,
    owner_is_main: bool,
    f: impl FnOnce() -> R + Send,
) -> R {
    #[cfg(test)]
    let origin = {
        let loc = std::panic::Location::caller();
        (loc.file(), loc.line())
    };
    super::owner_lane::exec_on_owner(owner, owner_is_main, || {
        // Recorded INSIDE the routed body, at the probe point, so a bare un-routed body
        // (wrapper bypassed) records on_lane = false. Same single-writer discipline as the
        // #949 redraw probe. The witness is the DISPATCH-GUARD marker: the dispatched arm
        // records true, a call nested in an on-lane block inherits the outer marker and also
        // records true, and the OS-main cold-thread inline arm runs WITHOUT a guard marker
        // and records false (expected — see the probe-semantics note below the module).
        #[cfg(test)]
        routing_probe::record(origin, super::owner_lane::on_owner_queue(owner));
        f()
    })
}

#[cfg(test)]
mod routing_probe {
    use std::sync::Mutex;
    static SINK: Mutex<Vec<((&'static str, u32), bool)>> = Mutex::new(Vec::new());
    pub(super) fn clear() { *SINK.lock().expect("..." ) = Vec::new(); }
    pub(super) fn record(origin: (&'static str, u32), on_lane: bool) {
        SINK.lock().expect("...").push((origin, on_lane));
    }
    pub(super) fn last() -> Option<bool> { SINK.lock().expect("...").last().map(|(_, on)| *on) }
    pub(super) fn all_on_lane() -> bool {
        SINK.lock().expect("...").iter().all(|(_, on)| *on)
    }
}
```

**Probe semantics (re-scoped in review).** The probe records the thread-local guard-marker VALUE at
the probe point — a dispatch-guard witness, not "this call installed a guard". Three arms:
(i) the dispatched off-lane route installs the guard and records `true` (the enforcement arm);
(ii) a call nested INSIDE another on-lane block runs inline via the reentrancy probe but inherits the
outer block's guard marker, so it ALSO records `true`;
(iii) the OS-main-thread shortcut on a cold thread (no outer block in scope) runs inline WITHOUT any
guard marker and records `false`. Arm (iii) recording false is correct, not a flaw: installing a
guard on the inline path is UNSOUND (a nested same-lane call would trip the same-owner
conditional-clear in `OnOwnerQueueGuard::drop` and self-deadlock on a serial lane), which is precisely
why `exec_on_owner` runs its direct paths bare. Test assertions therefore scope to the OFF-LANE arm
(the dispatched records, all `true`); a bare un-routed body (wrapper bypassed) records `false`.

Every swept class-A method body is rewritten to travel through it:

```rust
fn set_title(&self, title: String) {
    let owner = self.owner;
    let owner_is_main = self.owner_is_main;
    super::route_on_owner(owner, owner_is_main, || unsafe {
        // SAFETY: (see 2.5) the body runs under the owner-lane guard — inline on the OS main
        // thread for a main-lane owner, or dispatched onto the lane — before the message is sent.
        let title_ns: id = /* NSString::alloc(nil).init_str(&title) */;
        let _: () = msg_send![self.ns_window, setTitle: title_ns];
    });
}
```

Closures capture `&self` (Send via the existing `unsafe impl Sync for MacOSWindow {}`), never a raw
id — the liveness of the AppKit object is borrow-pinned across the synchronous hop, the established
#949 pattern.

**The exact class-A route set** (every body in `window.rs` whose method messages AppKit; builder
re-verifies body-by-body before wrapping, and any method that turned out AppKit-free is left
unwrapped and documented):

- `PlatformWindow` impl: `get_title`, `set_title`, `is_focused`, `is_visible`, `activate`,
  `minimize`, `maximize`, `restore`, `toggle_fullscreen`, `resize`, `close`, `set_cursor`.
- `request_redraw` — already owner-routed (#949); its body is rewritten to travel `route_on_owner`
  with the rest (ONE throat, ONE probe), and the #949 `redraw_thread_probe` module is folded into
  `routing_probe` (the window-level `request_redraw_is_owner_routed` test's probe assertions move
  onto the shared probe, re-scoped per the dispatch-guard semantics below; see §3).
- `WindowTrait` (12 bodies): `set_position`, `state`, `set_state`, `set_visible`, `is_resizable`,
  `set_resizable`, `is_minimizable`, `set_minimizable`, `is_closable`, `set_closable`,
  `set_min_size`, `set_max_size` (single-getter methods like `is_resizable` route too: their
  `styleMask` read IS an AppKit message; pure-Rust reads of the `state` mutex are exempt, and
  delegator bodies — `title`, `focus`, `is_focused`, `close`, `request_redraw`, `set_size` — are
  covered transitively by the PlatformWindow method they forward to).
- `MacOSWindowExtTrait` (`window_ext`, 11 bodies) impl: `set_liquid_glass_config`,
  `clear_liquid_glass`, `enable_tabbing`, `disable_tabbing`, `add_tab_to_window`,
  `toggle_native_fullscreen`, `set_window_level`, `window_level`, `set_collection_behavior`,
  `set_has_shadow`, `set_alpha`.

Routing `close()` does not change close-request callback delivery: the should-close consultation
round-trips through AppKit's `windowShouldClose:` delegate (class B, AppKit-delivered on main) into
`CloseRequestRouter::consult`, which **vetoes** a wrong-thread delivery before invoking the handler
(flui-app/app/close_request.rs:450-460). The routed programmatic `close` message is orthogonal to
that veto, so callback behavior is unchanged.

### 2.3 Fire-and-forget teardown primitive `exec_async_guarded` (owner_lane.rs)

Why the `Drop` tail is asynchronous-fire-and-forget and OWNED, never awaited or borrowed — three
library facts the builder must not fight (all verified against the pinned crates):

- **`Queue::exec_async` requires `F: 'static + Send + FnOnce()`** (dispatch 0.2, `queue.rs:157`),
  while `exec_sync` has **no** `'static` bound (`queue.rs:136-137` — what lets `route_on_owner`
  capture `&self`). An async lane body therefore CANNOT borrow `&mut self` from `Drop`'s stack; it
  must own its captures.
- **A borrowed async body is a use-after-free on the timeout path**: if an awaited async dispatch
  times out, the closure is still queued holding a reference to state `Drop` then deallocates.
  Owning the captures and NEVER WAITING removes that hazard outright — "teardown never hangs"
  (ADR-0045 decision 7) then holds BY CONSTRUCTION, with no timer, no spurious-timeout-under-load
  window, and no TOCTOU pre-check.
- **A raw `id` is `!Send`** (`cocoa::base::id` = `*mut Object`); an owned closure needs the
  lane-owned carrier wrapper defined in §2.4.

The helper — `exec_on_owner`'s guard + panic discipline applied to the `exec_async` path, which is
the only execution context a fire-and-forget teardown tail ever has:

```rust
/// Run `f` ON the owner lane, asynchronously, under the reentrancy guard and panic shield.
///
/// Unlike [`exec_on_owner`] this NEVER BLOCKS the caller: `f` is dispatched with `Queue::exec_async`
/// and the function returns immediately. Its purpose is teardown tails that must not hang window
/// close — `Drop` routes its AppKit tail here and lets the lane run it if and when it services. If
/// the lane is not servicing (pre-`run`, or the run loop is gone), `f` simply never runs and is
/// dropped WITH ITS CAPTURES — which is exactly why `f` must be `'static` and own its state. `f`
/// runs under the [`OnOwnerQueueGuard`] and is catch_unwind-wrapped (the GCD `exec_async` trampoline
/// is `extern "C"` with no panic catch, the same hazard `exec_on_owner`'s shield exists for); a
/// caught panic is logged here — fire-and-forget has no caller frame to resume to.
pub(super) fn exec_async_guarded(
    owner: &'static dispatch::Queue,
    f: impl FnOnce() + Send + 'static,
) {
    owner.exec_async(move || {
        let _guard = OnOwnerQueueGuard::new(owner);
        // SAFETY: the guard is installed before the body runs, so `on_owner_queue` reports on-lane
        // for the body's lifetime; the panic shield (a plain std call, infallible) wraps the body
        // before any unwind could cross GCD's C frames — the same argument as `exec_on_owner`'s.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        if let Err(payload) = r {
            tracing::warn!(
                "owner-lane teardown body panicked (swallowed; no caller frame to resume): {}",
                /* payload.downcast_ref::<&str>() / String — the workspace's usual panic-message helper */
            );
        }
    });
}
```

Design notes, decided:
- **Owner-owned, never borrowed**: the `'static` bound on `f` IS the design. A teardown tail owns
  its AppKit references; if the lane never runs it, the captures are dropped wherever the queue is
  torn down — anything borrowed from the dying `Drop` stack would dangle.
- **No semaphore, no bound, no timeout result**: the caller has nothing to do on "timeout" — the
  fallback IS the body never running (a leak), which needs no signal. A future caller that needs to
  know a routed-async body ran can bring its own semaphore and wait with its own bound; the helper
  stays minimal.
- **Always-run testable** (see §3): the shared `test_owner_queue` services, so a body dispatched
  through the helper runs under the guard — the drop-tail execution contract is pinned without any
  window, and the red-by-revert (removing the guard) is executable here.

### 2.4 `Drop` — route the AppKit tail, never block

Current `Drop` (window.rs:746-778) does, in order: `tracing::debug!(…)`; a11y `bridge.shutdown()`
(`#[cfg(feature = "a11y")]`, dereferences the content view — the #1194 residual); `windows_map`
Rust clean-up; `msg_send![ns_window, release]`. The last-`Arc` can be a wake-frame clone dropped on
an IO-lane worker after a routed `request_redraw` returns.

New shape: the AppKit tail is dispatched onto the owner lane via §2.3's `exec_async_guarded` and
**not awaited**. `Drop` does its Rust-only work synchronously, builds the tail's OWNED captures, and
returns immediately:

```rust
/// An Objective-C object id owned by the tail closure and only ever messaged ON its owner lane.
struct OwnerLaneId(id);
// SAFETY: `id` is `*mut Object`; a raw pointer is `!Send` because it generally carries no ownership
// or thread guarantee. This use provides both. The wrapper OWNS one outstanding retain (the window's
// balancing `release` is the ONLY message ever sent through it), and it is consumed either ON the
// owner lane — the object's home thread, where it was created — or dropped un-run, leaving the object
// over-retained (a leak, never a use-after-free). No code outside the owner lane ever dereferences
// it. This is the same ownership + affinity justification the file already makes for
// `unsafe impl Send for MacOSWindow {}`, scoped down to a single owned message.
unsafe impl Send for OwnerLaneId {}

impl Drop for MacOSWindow {
    fn drop(&mut self) {
        // Last-clone gate, unchanged (window.rs:746-778): ONLY the final `Arc` wrapper clone owns
        // teardown. Every earlier clone drop returns with no work — and MUST: a live clone's
        // accessibility bridge is still in service, and only one balancing `release` may ever be
        // sent (an unconditional tail from every clone would over-dealloc a live window).
        if Arc::strong_count(&self.state) != 1 {
            return;
        }
        tracing::debug!("Closing NSWindow {:p}", self.ns_window);

        // Rust-only clean-up, never routed (no AppKit contact, no lane):
        let window_id = self.ns_window as u64;
        self.windows_map.lock().expect("BUG: poisoned windows map").remove(&window_id);

        // The AppKit tail. Dispatched ONTO the owner lane and NOT awaited: `Drop` never blocks, so
        // teardown cannot hang (by construction, ADR-0045 decision 7), and the closure OWNS every
        // capture — nothing borrowed from this dying value crosses the lane (the `'static` bound on
        // `exec_async_guarded`). If the lane is un-servicing (pre-`run`, or the run loop is gone)
        // the closure never runs and both halves fall out soundly: the NSWindow is left
        // over-retained — never released, so no dealloc ever runs off-main — and the a11y adapter
        // leaks + warns via its own existing off-owner fallback (accessibility.rs:105-126).
        let owner = self.owner;
        let ns_window = OwnerLaneId(self.ns_window);
        #[cfg(feature = "a11y")]
        let a11y = self.accessibility.get().cloned(); // Option<Arc<...>>, Send (accessibility.rs:101)
        super::owner_lane::exec_async_guarded(owner, move || unsafe {
            // SAFETY: (see 2.5) the tail runs under the owner-lane guard — on-lane — so the a11y
            // unhook and the Window release both execute owner-affine. `shutdown()` runs before the
            // release in the same block, preserving accesskit's documented precondition (unhook the
            // dynamic subclass while the content view is still alive, accessibility.rs:71-75).
            #[cfg(feature = "a11y")]
            if let Some(a11y) = a11y {
                a11y.shutdown();
            }
            let _: () = msg_send![ns_window.0, release];
        });
    }
}
```

Design notes, decided:
- **The last-clone gate is load-bearing, not incidental (restored in review).** `Drop` returns with
  NO teardown when `strong_count != 1`, exactly as today. It carries two protections a fire-and-forget
  tail must keep: only the final wrapper's bridge may be shut down (an earlier clone's is still
  serving a live window), and only one `release` may ever run (the tail owns the window's single
  balancing +1 — an unconditional tail from every clone would over-release a live window). The gate
  wraps the WHOLE body — map removal, tail dispatch — not just the routed part, so no teardown work
  can run twice.
- **Route `release` too** (not just a11y): Apple documents the retain-count machinery thread-safe,
  but the dealloc it can drive (observer removal, responder teardown) is not documented off-main —
  routing the dealloc onto the lane is the sound reading, and keeps the whole tail uniform. Because
  the tail is fire-and-forget, the "escape" IS the body never running: the NSWindow is left
  over-retained and never deallocs off-main — there is no separate leak decision to make.
- **`windows_map` clean-up stays OUTSIDE the routed body**: it must not hold a Rust `Mutex` across
  the dispatch (a lane block waiting on the mutex after the drop-thread is gone is deadlock-shaped;
  and no AppKit is involved so the lane adds nothing).
- **`OwnerLaneId` is the one new `unsafe impl Send`** (a unit newtype around `cocoa::base::id`).
  Without it the owned tail closure cannot be `Send` and the design does not compile. Alternatives
  considered and rejected: moving the id as a `usize` and re-constructing on the lane (loses
  provenance — reviewer-rejected shape), and not routing `release` at all (leaves the
  dealloc-off-main hazard this change exists to close).
- **`exec_async_guarded` catches and LOGS panics** rather than resuming: fire-and-forget has no
  caller frame to resume to (that is what the synchronous `exec_on_owner` resume path is for). A
  panicking tail is logged on the lane and the process continues.
- **The earlier draft's bounded wait is DELETED.** It was TOCTOU-racy as a pre-check (isRunning →
  race → `exec_sync` blocks forever), and uncompilable — or use-after-free on timeout — as a
  bounded wait, because `exec_async`'s `'static` bound forbids borrowing `&mut self` from `Drop`'s
  stack. Owning the captures and never waiting removes every one of those hazards and is strictly
  simpler: no bound constant, no semaphore, no timeout arm, no leak-warning on a lane that merely
  drained late.

### 2.5 SAFETY prose, docs, and the load-bearing literals

- The `unsafe impl Send for MacOSWindow {}` / `unsafe impl Sync for MacOSWindow {}` **literals stay
  byte-identical** (lines 124/129 in window.rs; load-bearing conformance evidence). Their SAFETY
  prefaces (window.rs:86-129) are rewritten: delete "ONE mechanically-enforced site" and "Drop is a
  deliberate exception" claims; state honestly that the public surface is now mechanically
  owner-routed, AppKit-delivered callbacks run on main by construction, raw-handle accessors are
  debug-asserted with the enforcement upstream, and `Drop` fire-and-forgets its AppKit tail onto
  the lane (the un-run closure is the sound teardown leak). The whole SAFETY block (not just the
  diff's vicinity) is audited in this change — the #949 lesson: prose false in the SAME file as a
  mechanism change is a reviewer catch. The block must also carry the `OwnerLaneId` Send
  justification's sibling claim — that the lane-confined owned-`id` pattern is the SAME ownership +
  affinity argument the `unsafe impl Send for MacOSWindow {}` already makes, so the two SAFETY
  arguments stay mutually consistent.
- `request_redraw`'s method-doc title (currently "the ONE mechanically-enforced site in this file")
  is retitled to reflect that the surface is swept.
- `docs/runtime-contract.toml` `raster-wake-relay-precedes-thread-spawn`: the `CORRECTION
  (2026-09-16, issue #949)` text ("only that method's AppKit body is routed") is amended to state
  that the full public window surface is owner-routed as of #1194. The relay itself remains the
  open requirement: `state = "partial"`, `owner_issue = 559`, `mechanical = false` are retained, and
  both evidence literals (`unsafe impl Send for MacOSWindow {}`; `pub fn run_until_shutdown(mut
  self) {` in flui-engine) stay byte-untouched.
- `MacOSPlatform::app_path` is left **untouched** (folded out in review): its body is `NSBundle`
  singleton reads (thread-safe), and a debug assert there adds regression risk with no reachability
  signal (M3).
- Optional: the trait's `## Thread affinity` prose (traits/window.rs:95-121) gains a one-line note
  that the macOS backend enforces the default mechanically via `route_on_owner`. Only if the line is
  currently inaccurate — builder verifies first.
- No new `msg_send` sites are added; no public API changes; no new dependency.

## 3. Test strategy

- **Always-run, AppKit-free routing-wrapper pin (new, in window.rs):**
  `route_on_owner_runs_body_under_lane_guard` — a background thread calls
  `route_on_owner(test_owner_queue(), false, || ())` and returns; the assertion is that
  `routing_probe::last()` is `Some(true)` — the OFF-LANE arm's body observed the lane guard at the
  probe point. Per the probe semantics in §2.2, assertions scope to the off-lane arm: the OS-main
  cold-thread inline path intentionally runs without a guard marker and records `false`, and a nested
  on-lane call inherits the outer marker (records `true`) — so the always-run pin asserts ONLY the
  off-lane arm's `Some(true)`, never a nested call's record.
  **Red-by-revert verified by the orchestrator:** deleting the `exec_on_owner(…, || …)` wrapper from
  `route_on_owner` makes the off-lane probe record `false` → the test fails; the existing owner_lane
  `exec_on_owner_*` tests stay green. This is the executable "the sweep's wrapper routes" carrier —
  distinct from the owner_lane machinery tests, which pin `exec_on_owner` itself.
- **Existing owner_lane tests** (`exec_on_owner_runs_inline_when_already_on_the_lane`,
  `exec_on_owner_routes_off_lane_calls_onto_the_lane`) — unchanged, still the machinery pins.
- **Always-run, AppKit-free `exec_async_guarded` mechanism pin (new, in owner_lane.rs), so the
  Drop tail's execution contract is executable, not review-verified:**
  1. `exec_async_guarded_runs_body_under_lane_guard` — a background thread calls
     `exec_async_guarded(test_owner_queue(), body)` where the body records
     `on_owner_queue(test_owner_queue())` into a local `Arc<Mutex<Option<bool>>>`/semaphore and the
     caller waits on its own semaphore (with a test-side timeout, so a broken helper cannot hang the
     test); assert the recorded witness is `Some(true)` — the body ran under the lane guard on the
     normal path. **Red-by-revert (orchestrator-verified): deleting `OnOwnerQueueGuard::new` from the
     helper makes the witness `Some(false)` → the test fails** (the body still runs — dispatch is not
     what's pinned — but the guard is gone; this is the "the Drop tail body would execute under the
     guard" carrier).
  2. `exec_async_guarded_never_blocks_the_caller` — assert the helper RETURNS immediately even when
     `owner` is a **fresh serial `Queue` that nothing ever drains** (the body never runs; the helper
     returned with no wait) — the mechanical "`Drop` cannot hang teardown, by construction"
     (ADR-0045 decision 7) pin. Red-by-revert: if the helper ever waited on the lane this test would
     hang forever — caught by CI as a timeout.
- **Site-level, `#[ignore]`d (same honest reason as #949 — a bare `cargo test` process SIGABRTs
  NSWindow construction through `_CFBundleGetValueForInfoKey`; needs a run-loop-pumping test
  process):**
  1. Rename `request_redraw_is_owner_routed` to e.g. `window_surface_is_owner_routed` and extend its
     off-lane worker arm to drive **EVERY swept class-A body** — the full §2.2 route set (PlatformWindow
     12 + `request_redraw` + WindowTrait 12 + MacOSWindowExt 11) — one call per method from the worker
     thread, asserting each is crash-free AND `routing_probe::all_on_lane()` is true over the records
     the off-lane arm produced (an un-routed body records on_lane = false — the routing assertion,
     exactly the #949 probe shape). Because every driving call comes from the off-lane worker, ALL
     records are off-lane-arm records and the §2.2 inline-records-false caveat does not apply to the
     assertion. The `request_redraw` leg uses the shared probe post-consolidation (S2).
  2. New `drop_routes_appkit_tail_off_owner`: construct a real NSWindow via `for_test`, hand the last
     `Arc` to a background thread, drop it there, assert the drop RETURNS promptly (no hang — the
     no-block contract observed at site level) crash-free; the on-lane-ness of the tail body itself
     is covered by mechanism pin #1 (the Drop body is exactly the guarded body that pin runs). Under
     `feature = "a11y"`, `shutdown()` — which runs routed, on the lane, at owner-thread identity —
     takes the subclassing adapter out of the Mutex, so the adapter's own later `Drop` on the drop
     thread (wherever it is) hits the `None` arm at accessibility.rs:107 and returns silently: the
     off-owner leak fallback is exercised only if a window is dropped without ever shutting down
     (i.e. the tail closure never runs — a pre-`run`/teardown leak).
- **Honest disclosure (unchanged from #949):** site-level GREEN is NOT executable in a bare
  `cargo test` on this machine; the always-run `route_on_owner` + `exec_async_guarded` + owner_lane
  tests are the executable red-by-revert carriers; the `#[ignore]`d integration tests are the
  site-level anchors CI will run only when the macOS backend is ever linked there.

## 4. Environment

`crates/flui-platform` links AppKit locally → every linking cargo command uses
`--config 'build.rustc-wrapper="/tmp/lldstrip-wrapper.sh"'` (the committed `.cargo/config.toml`
appends `-fuse-ld=lld`, which this machine's clang 17 rejects; the wrapper strips only that arg).
`.cargo/` stays untouched (user constraint). `just runtime-conformance-check` needs
`PATH="/tmp/py312shim:$PATH"` (system python3 is 3.9 — no tomllib); `just ci` needs `/tmp/bashshim`
(bash 3.2 has no mapfile). `platform_it` 34 pass / 33 fail / 8 ignored on a live Mac is PRE-EXISTING
(ADR-0039 debug assert under libtest; identical on a stashed clean tree).

## 5. Verification (expected at build close)

- `cargo test -p flui-platform` (with the lldstrip wrapper) — 161 existing passed / 1 ignored (the
  #949 window test) → expect ~164 passed / 2 ignored: the new always-run wrapper pin +
  the two new `exec_async_guarded` mechanism pins add 3 to `passed`; the extended/renamed
  `window_surface_is_owner_routed` stays the one existing ignored test, and
  `drop_routes_appkit_tail_off_owner` is the second → / 0 failed. Headless suite 12 passed;
  `platform_it` pre-existing 34/33/8.
- `cargo clippy -p flui-platform --all-targets -- -D warnings` exit 0; `cargo fmt --check` clean;
  `cargo check` exit 0.
- `just runtime-conformance-check` exit 0 (56 contracts); both evidence literals byte-untouched.
- Red-by-revert (orchestrator-verified, per #949 practice): the always-run `route_on_owner` test
  fails exactly on the probe assertion when the wrapper's `exec_on_owner` is replaced with a bare
  `f()`; removing `OnOwnerQueueGuard::new` from `exec_async_guarded` fails its guard-witness pin.
- Honest disclosure: site-level (real-NSWindow) RED/GREEN for the swept methods and the Drop tail is
  not executable in a bare `cargo test` here — the always-run wrapper + mechanism pins and the
  ignored integration tests are the carriers; the Drop's no-hang contract is pinned mechanically by
  the `exec_async_guarded_never_blocks_the_caller` pin (§3), so the only review-verified claims left
  are that the Drop body (a11y unhook + release) is the right AppKit tail to route and that
  `OwnerLaneId`'s `unsafe impl Send` is sound — each anchored by the site-level `#[ignore]`d test
  and the SAFETY-prose audit (§2.5).

## 6. Explicitly NOT in this change

- The ADR-0045 D5 wake relay / `PlatformProxy` redraw verb — **#559/#551** (needs lane-less-backend
  transports); `raster-wake-relay-precedes-thread-spawn` stays `partial`.
- ADR-0039 slice 3: migrating `MacOSPlatform`'s affine entries onto the `!Send` `OwnerPlatform`
  capability (class C's recorded future; the debug assert is the slice-1 interim backstop).
- `view.rs`, `events.rs`, `display.rs`, `accessibility.rs` internals — untouched (event/delivery
  side; the a11y fix lands at `Drop`'s route, not in the adapter).
- Windows/Linux/Android backends.
- Per-method thread prose for all ~48 `PlatformWindow` methods (a separate docs concern, already
  rejected in #949 as not scaling).

## 7. Risk

- **Un-drained lane stall:** a routed class-A method called off-owner BEFORE the main lane services
  it blocks forever (`exec_on_owner` has no timeout; documented in owner_lane.rs). Resolved as
  non-reachable in practice: the only sanctioned off-owner callers (async frame-wake → request_redraw,
  surface-lease frame closure → un-routed raw-handle accessors, `Drop` → fire-and-forget, never a
  blocking lane call) all fire after `run` or never block. An unsanctioned pre-run off-owner call is
  a caller violation of the trait's `## Thread affinity` default and is the documented residual.
- **Cross-lane reentrancy deadlock:** unchanged — `route_on_owner` reuses `exec_on_owner`'s
  same-lane probe; never nest across lanes (module hazard documented in owner_lane.rs).
- **`Drop` teardown hang:** impossible by construction — the tail is `exec_async` (returns
  immediately; the always-run never-blocks pin tests this), so ADR-0045 decision 7 holds with no
  timer, no bound, and no AppKit call in the escape (there is no escape decision to make — the
  un-run closure IS the leak).
- **Late-release ordering:** the tail's `release` now runs asynchronously, after `Drop` returns and
  after the `windows_map` entry is already removed. Safe because the closure owns the only balancing
  `release` (nothing else can reference the window after the last `Arc` drops), and because a close
  path that needs the dealloc completed before returning does not exist in this backend.
- **OnceLock-identity dependence** of the reentrancy marker: unchanged, the statics stay.
- **`exec_async_guarded` swallows panics by design** — a Drop tail has no caller frame to resume;
  the panic is logged on the lane. This is a behavior choice (log, don't crash) made because the
  alternative (a panic unwinding through GCD's extern-"C" closure) is UB.
- **Honest limitation:** the sweep's per-site wrapping is proven at the wrapper level (always-run
  red-by-revert) + review; site-level GREEN is `#[ignore]`d (AppKit constraint, unchanged from #949).

## 8. Decision log (from the research synthesis)

- A route vs "route only the off-owner-reachable subset" — **full surface**: the subset reading
  classifies by a fragile call-graph fact; the surface is exactly GPUI's shape (Send+Sync Arc-shared
  wrapper), and Apple's doctrine grants no off-main safety to any of the 36 bodies.
- D route-then-leak vs "guard only the a11y shutdown" — **route the whole tail, fire-and-forget**
  (an awaited route is impossible: `exec_async` is `'static`-bound and a borrowed body is a
  use-after-free on the never-serviced lane): guarding only the a11y half leaves the
  `release`→dealloc path off-main, and routing the tail is the sound reading of
  dealloc-not-documented-off-main; the un-run closure IS the teardown leak, which preserves the
  no-hang rule with no timer.
- E document vs route — **document + assert**: `RawWindowHandle` is `!Send`, cannot cross
  `exec_on_owner<R: Send>`; downstream enforcement (raw-window-metal `MainThreadMarker`) already
  forces the main thread before surface creation.
- C keep-assert vs route — **keep assert**: ADR-0039 slice 1's recorded decision; `quit` is NOT
  cross-thread reachable (`OwnerPlatform` is `!Send`, owner-confined), so `terminate:` stays off the
  routed surface and the assert is the slice-1 interim backstop (rationale corrected in review); the
  slice-3 OwnerPlatform capability migration is the mechanism's future.
- **Reviewer folds (Phase 2.5 — all three verdicts `RESHAPE NEEDED`, folded into §1-§5):**
  - The **bounded-wait teardown** (earlier §2.4 draft) is DELETED — see the "earlier draft's bounded
    wait is DELETED" note in §2.4; the fire-and-forget owning tail is the design all three reviewers
    reviewed.
  - **Last-clone gate restored (critical):** `Drop` unconditionally dispatching the tail from EVERY
    wrapper clone was over-release for live clones and an off-owner `shutdown()` on a live bridge;
    the `Arc::strong_count(&self.state) == 1` gate (today's shape, window.rs:746-778) is restored
    around the whole body (§2.4).
  - **Probe re-scoped (refined by the Phase 5b code review):** the probe is a dispatch-guard witness,
    not "this call installed the guard": the dispatched arm installs the guard and records `true`; a
    nested same-lane call inherits the outer block's marker and also records `true`; the OS-main
    cold-thread inline arm records `false` (no marker) — and must not install a guard (same-lane
    self-deadlock). Test assertions stay scoped to the off-lane arm (§2.2, §3); tests never assert on
    nested inline calls.
  - **`app_path` assert dropped:** NSBundle reads are thread-safe; the planned assert was scope creep
    with regression risk (§2.1, §2.5).
  - **`quit` rationale corrected:** not "documented cross-thread reachable" — `OwnerPlatform` is
    `!Send` (owner-confined); the assert is slice-1 precedent (§2.1).
  - **Class-E rationale corrected to the verified facts:** raw-window-metal 1.1.0 `Layer::from_ns_view`
    HARD-panics off-main (lib.rs:403), while rwh 0.6.2 `AppKitWindowHandle` is marker-free/documented;
    E keeps assert+document, residual is the single un-routed `contentView` accessor (§2.1).
  - **`close()` routed, delivery unchanged:** `CloseRequestRouter::consult` vetoes wrong-thread
    deliveries itself (flui-app close_request.rs:450-460) (§2.2).
  - **`request_redraw` consolidated** onto `route_on_owner`; `redraw_thread_probe` folds into
    `routing_probe` (S2) (§2.2, §3).
