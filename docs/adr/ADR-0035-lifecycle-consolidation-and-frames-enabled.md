# ADR-0035: Lifecycle consolidation and the frames-enabled re-enable leg

*The workspace had three separate app-lifecycle representations. `flui_scheduler::Scheduler`'s `AppLifecycleState` — the one actually wired to `frames_enabled` — becomes the single canonical one; the other two are deleted, not merely deprecated. Its `handle_app_lifecycle_state_change` also gains the Flutter parity leg it was missing: re-scheduling a frame on the disabled→enabled edge, not just toggling a flag nothing then acts on.*

---

- **Status:** Accepted
- **Date:** 2026-07-18
- **Deciders:** @vanyastaff
- **Scope (PR1 — this change):** `crates/flui-scheduler/src/scheduler.rs` (`Scheduler::handle_app_lifecycle_state_change`, `should_schedule_frame`, deleted `should_run_animations`); `crates/flui-view/src/binding.rs` (`AppLifecycleState` re-export); `crates/flui-platform/src/traits/lifecycle.rs` (deleted); `crates/flui-app/src/app/{lifecycle.rs (deleted), binding.rs, runner.rs}`
- **Related:** ADR-0027 (owner-affine `UiRealm`s — the realm-dispatch owner-thread discipline this ADR's debug_asserts lean on); ADR-0029 (frame pacing — `no_present_fallback_pace`, the throttle this ADR's re-enable leg complements rather than replaces); ADR-0034 (clipboard reachability — the `Box<dyn Platform>`-consumed-by-`run()` bootstrap shape this ADR's Started/Terminating calls slot into)

---

## Context

Three independent representations of "is the app running" existed in the workspace at once:

1. **`flui_scheduler::AppLifecycleState`** (`frame.rs`) + `Scheduler::handle_app_lifecycle_state_change` (`scheduler.rs`) — a real Flutter port. Five variants (`Resumed`/`Inactive`/`Hidden`/`Paused`/`Detached`) matching Flutter's `AppLifecycleState` exactly, and a handler that auto-toggles a `frames_enabled: AtomicBool` per `binding.dart:414-441` (Resumed/Inactive keep frames on; Hidden/Paused/Detached turn them off), plus a `CallbackId`-keyed listener registry with clone-out-of-lock dispatch and change-only notification. This is the only one of the three actually read by the frame pipeline (`should_schedule_frame`/`frames_enabled`).
2. **`flui_view::AppLifecycleState`** (`binding.rs`, marked `PORT-CHECK-OK-SP3`) — a parallel five-variant enum with the identical variant names, used only as the parameter type of `WidgetsBindingObserver::did_change_app_lifecycle_state` and `WidgetsBinding::handle_app_lifecycle_state_changed`. Nothing outside `flui-view` itself constructed or dispatched it — the observer plumbing existed with no production caller feeding it a state.
3. **`flui_platform::traits::{LifecycleState, LifecycleEvent, PlatformLifecycle, DefaultLifecycle}`** — a *different* state shape (`Starting`/`Active`/`Inactive`/`Background`/`Terminating`, driven by discrete `LifecycleEvent`s rather than direct state values) that `AppBinding` instantiated as `lifecycle: Mutex<DefaultLifecycle>` and exposed via `lifecycle_state()`/`transition_lifecycle()`/`should_render()`. This is what the runner's bootstrap/shutdown and window-focus callbacks actually drove — and it fed nothing: `AppBinding::should_render()` had zero callers outside its own definition, so the whole state machine computed a fact nobody read.

The result: the fact the frame pipeline actually needs (`frames_enabled`) lived on representation #1, but the runner's lifecycle *events* (platform quit, bootstrap started, window focus) drove representation #3, and representation #2 sat fully disconnected in the middle. A platform quit or window-focus change updated a state machine that could not affect frame scheduling even in principle.

Separately, representation #1's handler had a real parity gap even on its own: it flips `frames_enabled` false→true on Resumed/Inactive, but never calls anything that produces a frame. Flutter's `_setFramesEnabledState(true)` (`binding.dart` @ 3.44.0) calls `scheduleFrame()` on exactly that transition. Without it, an app that comes back from Hidden/Paused never wakes an idle event loop — the flag flips, nothing re-requests a frame that was never scheduled while frames were off, and the app appears frozen until an unrelated input event happens to nudge it.

## Decision

### 1. Consolidate onto `flui_scheduler::AppLifecycleState` — delete, don't deprecate

Representation #1 is canonical because it is the one tied to real behavior (`frames_enabled`, the listener registry). Representations #2 and #3 are deleted outright, not aliased-and-kept:

- `flui_view::binding::AppLifecycleState` (the enum) is replaced by `pub use flui_scheduler::AppLifecycleState;`. `flui-view` already depends on `flui-scheduler` (no new edge). `WidgetsBindingObserver::did_change_app_lifecycle_state` and `WidgetsBinding::handle_app_lifecycle_state_changed` keep their signatures — the type name is unchanged at every call site, only its origin crate changes. The `PORT-CHECK-OK-SP3` marker is removed with the duplicate it sanctioned.
- `flui_platform::traits::lifecycle` (`LifecycleState`, `LifecycleEvent`, `PlatformLifecycle`, `DefaultLifecycle`) is deleted wholesale: zero production consumers read `PlatformLifecycle::should_render()`/`is_focused()`/`is_visible()`, and the one real consumer (`AppBinding`) is being retired in the same change (below).
- `AppBinding`'s `lifecycle: Mutex<DefaultLifecycle>` field and its `lifecycle_state()`/`transition_lifecycle()`/`should_render()` methods are deleted. `flui-app/src/app/lifecycle.rs` (the re-export shim) is deleted with them.

A type that exists in two crates because "it might diverge later" is exactly the SP-3 smell `scripts/port-check.sh` trigger 10 flags: two names for one concept invite the concept to drift, and here it already had — representation #3 used completely different variant names and semantics from #1/#2 despite modeling the same thing.

### 2. The runner drives the scheduler directly, not through a `RealmEvent` round-trip

Before this change, `RealmEvent::Lifecycle(LifecycleEvent)` was one of the variants `dispatch_platform_realm` queued and ran on the realm's owner thread, alongside `Input`/`Resize`/`Active`/`Frame`. That indirection existed to serialize lifecycle changes with in-flight frame/input dispatch — but neither `Started` nor `Terminating` needs `&UiRealm` access (the `Lifecycle` arm of `RealmEvent::run` never touched its `realm` parameter), and both call sites (`platform.on_quit`, and the post-bootstrap "mark as started" call) already run on the owner thread by construction: `on_quit` fires from the platform event-loop thread, and bootstrap itself runs on it.

`RealmEvent::Lifecycle` is deleted. `bootstrap_desktop`, `run_android`, and `run_web` now call `Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Detached)` directly from `on_quit`, and `handle_app_lifecycle_state_change(AppLifecycleState::Resumed)` directly right after the window is stored — no `RealmEvent`, no `dispatch_platform_realm` round-trip. Each call site carries a `debug_assert_eq!(std::thread::current().id(), realm_dispatch.owner_thread, ...)` immediately before it: `flui-app` already has the owner-thread identity cheaply in scope (`RealmDispatcher::owner_thread`, captured in the same closures that used to build the `RealmEvent`), so the assertion belongs at the call site, not on the scheduler — `Scheduler` itself has no notion of "realm" or "owner thread" to check against, and inventing one there only to support one caller's debug assertion would be a bigger, cross-crate change for a check `flui-app` can already make for free. `Scheduler::handle_app_lifecycle_state_change`'s doc comment states the contract instead: listener callbacks fire synchronously on whatever thread calls in; production callers are expected to already be on the realm's owner thread.

`RealmEvent::Active(bool)` (window focus/active-status changes) is **not** deleted — it keeps existing as a dispatched event — but its handler body is now an intentional no-op for this PR. It used to convert the bool into a `LifecycleEvent::Activated`/`Deactivated` and feed `AppBinding::transition_lifecycle`; since that whole state machine is gone, honestly reflecting "focus changes do not yet feed any lifecycle signal" beats quietly wiring it into something it was never designed to drive. The `(visible, focused)` derivation that turns focus changes into a real `AppLifecycleState` transition is PR2's job (see Deferred, below) — inventing a partial version of it here, just to keep `Active`'s old call shape, would be exactly the kind of behavior wiring this PR is scoped to exclude.

### 3. The re-enable leg: schedule a frame on the disabled→enabled edge

`handle_app_lifecycle_state_change` now captures the *previous* value of `frames_enabled` (via `AtomicBool::swap`, not `load`-then-`store`, so the read-and-flip is one atomic op) and compares it against the newly computed value. Only on the false→true edge does it call `self.request_frame()` — the same method `schedule_frame()`/`spawn_local`'s wake path already use, so `on_frame_scheduled` fires exactly like any other real schedule, not a bespoke wake mechanism. Every other transition (same state repeated; Resumed↔Inactive, which never changes `frames_enabled`; the enabled→disabled edge) schedules nothing, matching Flutter's `_setFramesEnabledState`, which only calls `scheduleFrame()` on the false→true edge.

This closes the concrete symptom: a `Hidden`/`Paused`/`Detached` app that becomes `Resumed`/`Inactive` again now always produces a frame request, rather than depending on some unrelated input/resize event to happen to wake the loop next.

### 4. Duplicate-fact cleanup: `should_schedule_frame` and `should_run_animations`

`should_schedule_frame()` re-derived its answer from `lifecycle_state().should_render()` — a second, independent computation of the same fact `frames_enabled` already tracks (and that `handle_app_lifecycle_state_change` keeps in sync on every transition). It is now a thin alias over `frames_enabled()`: one source of truth, not two expressions that happen to currently agree.

`should_run_animations()` (`Resumed`-only) had zero callers outside its own unit test, and encodes a semantic Flutter does not actually have at the scheduler layer — Flutter does not gate ticker muting purely on lifecycle state at `SchedulerBinding`; that is a `TickerMode`/widget-tree concern. A zero-consumer method whose gating rule does not correspond to real Flutter behavior is deleted rather than kept as an attractive nuisance for a future caller to pick up and rely on. `AppLifecycleState::should_animate()` (the enum-level predicate `should_run_animations()` wrapped) is *not* deleted — it has a real caller (an integration test) and is one of several intentional Rust-native query methods (`is_visible`/`is_focused`/`can_animate`/`should_render`/`should_save_state`/`should_release_resources`) already documented elsewhere as additions beyond the base Flutter port, not a duplicate of anything.

## Named divergences (carried forward, not fixed by PR1)

- **No retained scene ⇒ no automatic re-dirty on resume.** Flutter's engine keeps the last rendered layer tree around while paused, so resuming can present stale pixels immediately while a real frame catches up. FLUI has no such retained-scene layer at the app level yet, so becoming visible again needs an explicit re-dirty (an app-level "root changed, please rebuild" signal) alongside the frame this PR's re-enable leg now guarantees gets scheduled. That derivation is PR2's job, tied to the `(visible, focused)` work below — this PR only guarantees a frame *runs*, not that it necessarily has fresh content on the very first one after a long hide.
- **FLUI polls async work inside the frame, not via a background reactor.** `Scheduler::drive_frame`'s async-driver step polls pending futures once per frame, on the frame thread. When frames are disabled (`Background`/`Hidden`/`Paused`/`Detached`), nothing drives that poll — a future that would otherwise complete (a network response callback resuming widget state) sits unpolled until frames re-enable. Flutter's Dart event loop keeps running microtasks/timers regardless of `AppLifecycleState`; FLUI's is coupled to the frame loop. A `PumpAsync` explicit pump for the backgrounded case is PR2 scope, not this one — PR1 only fixes the *frame-scheduling* gap, not the async-polling-while-backgrounded gap, which is a materially different mechanism (a pump with no frame attached, versus a frame that now reliably gets requested).

## What PR2 wires (deliberately not in this PR)

This PR is consolidation + the scheduler-internal parity leg only — no platform-facing behavior changes:

- Winit's `WindowEvent::Occluded` (or the equivalent per-backend visibility signal) wired to a visibility callback that feeds the lifecycle derivation, rather than lifecycle staying keyed only to window focus as it is today.
- A real `(visible, focused)` → `AppLifecycleState` derivation, replacing `RealmEvent::Active`'s current no-op body with the actual mapping (`Resumed` when visible+focused, `Inactive` when visible-but-unfocused, `Hidden` when not visible but still running, etc.).
- Ladder synthesis: a visibility/focus transition that skips intermediate states (e.g., going straight from `Resumed` to `Hidden` without ever seeing `Inactive`) needs to synthesize the intermediate step(s) Flutter's binding would have produced, not jump straight to the end state.
- The `wake_action` gate: deciding *whether* a wake actually needs to run a frame versus just flip a flag, once visibility is a real signal and not just "we happened to get a quit/started event."
- Android's Pause ladder (`onPause`→`onStop`→`onDestroy` mapped to the right intermediate `AppLifecycleState` values, not collapsed to a single transition).
- `WidgetsBindingObserver` dispatch: today `flui-view`'s `handle_app_lifecycle_state_changed` exists with no production caller feeding it a state at all (per the Context section) — PR2 is what actually calls it from the real transitions PR2 introduces.

## Verification honesty

- **Wayland has no automated occlusion test.** The `Occluded`/visibility signal PR2 will consume is a compositor protocol extension without a portable way to simulate a window being covered in an automated test on Linux/Wayland; that verification will stay manual (documented, not automated) when PR2 lands it.
- **Windows minimize-state deferral.** Windows reports minimized/restored through its own message pump semantics distinct from simple occlusion; mapping that onto the same `(visible, focused)` derivation PR2 introduces needs its own verification pass against a real Windows session, deferred to when PR2's derivation lands, not asserted here.

## Deferred (not this PR, not necessarily PR2 either)

- A dedicated Android lifecycle callback distinct from the generic window active-status hook `run_android` already has.
- Multi-window hidden-state aggregation (today's single-window assumption: one window's visibility is the whole app's visibility; a multi-window app needs "all windows hidden" aggregation before reporting `Hidden`).
- `onExitRequested`-style negotiated-exit (the app getting a chance to veto or delay a platform-initiated exit) — today `Terminating`/`Detached` is a one-way notification.
- A widget-tree-facing lifecycle capability (a `BuildContext` seam a widget could use to read/observe lifecycle state directly, analogous to the `text_input_handle()` precedent) — no consumer exists yet to justify the seam.

## Alternatives rejected

- **Extend `flui_platform::DefaultLifecycle` into the canonical type instead of `flui_scheduler::AppLifecycleState`.** Rejected: it would create a *fourth* representation (a modified #3) rather than resolving the existing three down to one, and #3's variant shape (`Starting`/`Active`/`Inactive`/`Background`/`Terminating`, event-driven) does not match Flutter's `AppLifecycleState` naming or transition model at all — extending it forces every future call site to keep translating between "the Flutter-named concept everyone actually means" and this crate-local shape.
- **Gate `frames_enabled` before `Scheduler::drive_frame` runs, rather than at the lifecycle-change call site.** Rejected: `drive_frame` is the single frame-execution entry point both `AppBinding::render_frame_entered` and `HeadlessBinding` share; a gate placed there would silently stop polling the async driver too (the mid-frame slot lives inside `drive_frame`), starving any in-flight future the moment frames disable — exactly the async-polling gap this ADR names as a divergence, not something to make worse by construction. Toggling `frames_enabled` at the point the lifecycle actually changes, and letting `schedule_frame_if_enabled`/`should_schedule_frame` gate whether a frame is requested in the first place, keeps the async-driver question separate from the frame-scheduling one instead of conflating them.
- **Skip only the presentation step while backgrounded, but keep running full frames.** Rejected: this still burns pipeline CPU (build/layout/paint) every vsync for a window nobody can see, which is precisely the resource cost `frames_enabled` exists to avoid. A present-only skip trades a real problem (no visible feedback while backgrounded, addressed by the retained-scene divergence above) for a fake fix that keeps the actual waste running.

## Evidence

- `cargo nextest run --workspace --exclude flui-platform`: passes.
- `cargo test -p flui-platform --lib`: passes (55 tests; flui-platform's own suite is excluded from the workspace nextest gate per `AGENTS.md`'s "Testing quirks").
- `cargo clippy --workspace --all-targets -- -D warnings` (not `--all-features`): clean.
- `cargo check -p flui-app --target wasm32-unknown-unknown`: compiles.
- `just fmt-check port-check inventory-check`, `taplo fmt --check`, `typos`: clean.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-scheduler -p flui-view -p flui-platform -p flui-app --no-deps --document-private-items`: clean.
- Doctests (`cargo test --doc` for the four crates above): pass.
- Red→green evidence: temporarily short-circuiting the `frames_were_enabled`/`should_render` edge check in `handle_app_lifecycle_state_change` (forcing it to never fire) made `lifecycle_reenable_edge_schedules_exactly_one_frame` fail; restoring the real edge check turned it green again. `lifecycle_resumed_to_inactive_schedules_nothing` and `lifecycle_repeated_same_state_schedules_nothing` cover the two "must NOT schedule" cases from the same edge logic.
