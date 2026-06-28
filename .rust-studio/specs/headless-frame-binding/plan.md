# Headless frame-driving binding — deterministic `pump_frame(dt)`

Status: **DESIGN (read-only)** — 2026-06-26. Author: chief-architect.
Scope: a minimal, deterministic, per-frame driver that unblocks time-based features
(long-press / double-tap / implicit animations) in headless tests.

> Pre-code maintainer gate verdict (design): **ACCEPTABLE** for the phased plan below.
> One reshape condition is load-bearing: do **not** make the long-press deadline
> deterministic by special-casing the test (no `cfg(test)` clock read, no injected
> `now` that only the test sets) — virtualize the *production* time source so the live
> app and the headless test traverse the same code. See §1.

---

## 0. Problem, restated against the verified code

Per-detector `GestureDetector` (tap + drag) and `AnimatedView` (explicit `tick_at`)
work. Every *remaining* time-based feature is blocked because nothing advances a clock,
ticks controllers, or polls arena deadlines **per frame** in the headless model.

Two concrete, verified obstacles:

- **Long-press deadline is wall-clock.** `LongPressGestureRecognizer::handle_down`
  (`crates/flui-interaction/src/recognizers/long_press.rs:271`) captures
  `down_time = Some(Instant::now())`, and `try_fire_timer` (`long_press.rs:423`) fires
  when `Instant::now().duration_since(down_time) >= long_press_duration()`. The frame
  poll *exists* — `GestureArenaMember::poll_deadline` → `check_timer` → `try_fire_timer`
  (`long_press.rs:604`) — and `GestureArena::poll_deadlines()`
  (`crates/flui-interaction/src/arena/mod.rs:962`) iterates members and calls it. But
  because both the capture and the comparison read `Instant::now()`, the only way a test
  fires the deadline today is `std::thread::sleep(550ms)` (see the three
  `std::thread::sleep` sites in `long_press.rs` tests, incl.
  `held_pointer_fires_long_press_via_arena_poll` at `long_press.rs:889`). That is the
  nondeterminism the runtime forbids.

- **Implicit animations need a per-frame controller advance.** `AnimationController::tick_at`
  (`crates/flui-animation/src/controller.rs:787`) is the single deterministic time entry,
  but it is reached today only by tests calling it by hand. The auto-ticker
  (`Ticker::tick_and_reschedule_static`, `crates/flui-scheduler/src/ticker.rs:643`) reads
  `start.elapsed()` (wall-clock) and *ignores* the `_vsync_time` it is handed — so driving
  animations through `Scheduler::handle_begin_frame(now)` would **not** be deterministic.

What already exists and must be reused (not rebuilt):

| Seam | Location | Role in `pump_frame` |
|---|---|---|
| `Scheduler::handle_begin_frame(vsync)` / `add_persistent_frame_callback` | `crates/flui-scheduler/src/scheduler.rs:527,805` | NOT on the deterministic path (auto-ticker is wall-clock); used only if/when the ticker clock is virtualized (deferred, §4). |
| `AnimationController::tick_at(raw_elapsed_secs)` | `controller.rs:787` | The deterministic controller advance. |
| `GestureArena::poll_deadlines()` | `arena/mod.rs:962` | The per-frame deadline sweep. Signature is preserved. |
| `GestureBinding::tick_deadlines()` / `flush_pending_moves()` | `crates/flui-interaction/src/binding.rs:742,512` | Already wrap `poll_deadlines` + coalesced-move flush; the binding orchestration calls these on the *shared* arena (§3). |
| `BuildOwner::build_scope` draining `external_inbox` | `crates/flui-view/src/owner/build_owner.rs:418,438` | Animation/listenable ticks land in the inbox; `build_scope` drains them. Already wired. |
| `PipelineOwner::run_frame()` | used at `crates/flui-app/src/bindings/renderer_binding.rs:434` and `crates/flui-widgets/tests/common/mod.rs:92` | Layout/paint/composite. |
| Harness `LaidOut::pump()` / `tick()` | `crates/flui-widgets/tests/common/mod.rs:156,175` | Today: `build_scope` + `run_frame`. Neither advances a clock, ticks controllers, nor polls deadlines. `pump_frame` is their superset. |

---

## 1. Deterministic headless clock (the central decision)

### Options

- **(A) Inject a monotonic clock the gesture system reads** — recognizers read
  `clock.now()` for both down-capture and deadline-check; the frame driver advances the
  clock. Production default reads `Instant::now()` (zero behavior change).
- **(B) Push `now` through the poll** — change `poll_deadline(&self)` →
  `poll_deadline(&self, now)` and give `add_pointer`/down-capture a time argument; the
  binding threads a virtual `now`.
- **(C) Tiny timeout + real sleep** — what tests do today. Rejected: nondeterministic,
  slow, and forbidden by the runtime's no-wall-clock rule.

### Decision: (A), with the clock **owned by the `GestureArena`**

Rationale (why A over B, and why arena-owned):

1. **The arena is already the shared coordination object.** Every recognizer holds it via
   `RecognizerBase` (`long_press.rs:104` → `state.arena`), and `poll_deadlines` is the
   arena's own method. Putting the clock there localizes the injection to **one**
   constructor and leaves `poll_deadline()` / `poll_deadlines()` signatures **unchanged**
   — so `GestureBinding::tick_deadlines()` and every existing call site are untouched.
   Option (B) churns the `GestureArenaMember` trait, `add_pointer` (which has no time
   param today: `add_pointer(&self, pointer, position)`), and every `GestureDetector`
   call site.
2. **It mirrors a pattern already in the codebase.** `AnimationController` holds an
   `Arc<Scheduler>` as its injected time authority (`controller.rs:191`). A clock-on-arena
   is the gesture analogue: a time authority injected once at construction.
3. **Production is provably unchanged.** Default clock = a `SystemClock` whose
   `now()` is `Instant::now()`. The live app and current tests keep their exact behavior;
   only the headless binding swaps in a `ManualClock`.
4. **`Instant` stays the currency.** `ManualClock { base: Instant, elapsed: Arc<Mutex<Duration>> }`
   with `now() = base + *elapsed.lock()` returns real `Instant`s on a virtual timeline, so
   `LongPressState::down_time: Option<Instant>` (`long_press.rs:145`) needs **no type
   change** — only its *source* changes from `Instant::now()` to `self.state.now()`.

This is the FLUI-native equivalent of Flutter's `FakeAsync`: Flutter virtualizes the
`Timer`/`Stopwatch` clock that `_timer = Timer(deadline, didExceedDeadline)`
(`.flutter/.../gestures/recognizer.dart:708`) runs against; FLUI virtualizes the `now()`
that the poll-driven deadline reads. Same idea — replace the wall-clock source, not the
mechanism.

### Shape (flui-interaction)

```rust
// crates/flui-interaction/src/clock.rs  (NEW, ~60 LOC)

/// Monotonic time source for deadline-driven gesture recognition.
/// Production reads the OS clock; headless/tests advance a virtual clock so a
/// long-press deadline elapses deterministically with no wall-clock sleep.
pub trait MonotonicClock: Send + Sync + std::fmt::Debug {
    fn now(&self) -> web_time::Instant;
}

/// Real clock — `Instant::now()`. The default; production behaviour is unchanged.
#[derive(Debug, Default)]
pub struct SystemClock;
impl MonotonicClock for SystemClock {
    fn now(&self) -> Instant { Instant::now() }
}

/// Virtual clock advanced explicitly by the frame driver. `now()` is
/// `base + elapsed`; `advance(dt)` moves the timeline forward.
#[derive(Debug, Clone)]
pub struct ManualClock { base: Instant, elapsed: Arc<Mutex<Duration>> }
impl ManualClock {
    pub fn new() -> Self { Self { base: Instant::now(), elapsed: Arc::new(Mutex::new(Duration::ZERO)) } }
    pub fn advance(&self, dt: Duration) { *self.elapsed.lock() += dt; }
}
impl MonotonicClock for ManualClock {
    fn now(&self) -> Instant { self.base + *self.elapsed.lock() }
}
```

Arena change — additive, default preserves behavior:

```rust
// crates/flui-interaction/src/arena/mod.rs
pub struct GestureArena {
    entries: Arc<DashMap<PointerId, Mutex<ArenaEntryData>>>,
    clock: Arc<dyn MonotonicClock>,   // NEW
}
impl GestureArena {
    pub fn new() -> Self { Self::with_clock(Arc::new(SystemClock)) }      // unchanged behavior
    pub fn with_clock(clock: Arc<dyn MonotonicClock>) -> Self { /* ... */ }
    pub fn now(&self) -> Instant { self.clock.now() }                     // NEW, read by recognizers
}
```

`RecognizerBase` gains `fn now(&self) -> Instant { self.arena.now() }`; `long_press.rs`
replaces its two `Instant::now()` reads (`:271`, `:423`) with `self.state.now()`. No other
recognizer needs the clock yet (double-tap will, when it lands — §6).

> **Reshape guard (maintainer bar):** the change is a behavior-preserving substitution of
> the time *source*. The red→green test (§5) must fail on `main` (sleep-free, the deadline
> never fires) and pass after — proving the virtual clock actually drives the deadline, not
> a test-only shortcut. `ManualClock` must be reachable from production types (it is — it
> impls the public `MonotonicClock`), never `#[cfg(test)]`-gated.

---

## 2. `pump_frame(dt)` — where it lives and the ordering

### Where it lives — recommendation + the one fork to confirm

This is a **new-crate-vs-in-place** fork (escalation-worthy). Recommendation:

> **Introduce an instantiable, non-singleton `HeadlessBinding` struct** that owns the
> deterministic clock and the frame orchestration. Start it as a `pub` test-support type;
> the existing `LaidOut` harness (`tests/common/mod.rs`) becomes a thin wrapper that
> constructs one. Do **not** bolt the orchestration onto the singleton `GestureBinding`
> (per-detector arenas are disjoint from it — §3) or the singleton
> `RenderingFlutterBinding` (it is a process-global; CI already runs widget tests
> `--test-threads=1` around exactly this kind of shared state — see `AGENTS.md` Testing
> Quirks).

Crate placement — pick one (recommend **B1**, fall back to **B2**):

- **B1 — a new lightweight `flui-binding` crate** between `flui-widgets` and `flui-app`,
  depending on `flui-{scheduler,animation,interaction,view,rendering}`. Pro: a real reusable
  home that can grow toward a production headless/embedder binding; keeps flui-widgets'
  surface clean. Con: a new crate (weigh against the small-crate cost — justified here by a
  clear cross-domain boundary the orchestration already spans).
- **B2 — `flui-app` behind a `testing` feature**, as a non-singleton sibling of `AppBinding`.
  Pro: bindings already live in `flui-app`. Con: flui-app pulls winit/platform weight and has
  the documented singleton flake; a headless binding wants to be light and parallel-safe.

The only **mandatory** cross-crate primitive is the clock (§1, lands in `flui-interaction`
regardless). The orchestrator can begin life in `flui-widgets/tests/common/mod.rs` and be
**promoted to the chosen crate on the second consumer** (rule of three) — so Phase 1 does
not block on the crate decision.

### The `pump_frame` contract

```rust
pub struct HeadlessBinding {
    clock: ManualClock,                          // §1 — the single time authority
    arena: GestureArena,                         // shared, clock-bound (§3)
    gesture: /* GestureBinding or just the arena */,
    controllers: Vec<AnimationController>,        // explicitly registered (§4)
    build_owner: BuildOwner,
    tree: ElementTree,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl HeadlessBinding {
    /// Advance one deterministic frame by `dt`.
    pub fn pump_frame(&mut self, dt: Duration) {
        // 1. Advance the virtual clock. Everything time-based reads from here.
        self.clock.advance(dt);

        // 2. Fire gesture deadlines at the NEW time (Flutter: timers fire during
        //    `fakeAsync.elapse(dt)`, BEFORE the frame). May invoke user callbacks
        //    that setState -> schedule into the build inbox.
        self.arena.poll_deadlines();             // = GestureBinding::tick_deadlines()
        // (optional) self.gesture.flush_pending_moves();  // if input was queued

        // 3. Advance registered animation controllers deterministically.
        //    notify_listeners -> AnimatedView mark-dirty -> external_inbox.
        let elapsed = self.clock_elapsed_secs();
        for c in &self.controllers { c.tick_at(elapsed_for(c)); }   // see §4 on run-epoch

        // 4. Drain the build inbox + reconcile. build_scope() drains external_inbox
        //    at its start (build_owner.rs:438), so steps 2-3 are already visible.
        self.build_owner.build_scope(&mut self.tree);

        // 5. Run the pipeline frame (layout/paint/composite).
        let mut g = self.pipeline_owner.write();
        let owner = std::mem::take(&mut *g);
        let (owner, result) = owner.run_frame();
        result.expect("headless frame should succeed");
        *g = owner;
    }
}
```

### Ordering rationale vs Flutter `SchedulerBinding` / `GestureBinding`

Flutter `TestWidgetsFlutterBinding.pump(dt)` (verified at
`.flutter/.../flutter_test/lib/src/binding.dart:2249`):

```
fakeAsync.elapse(dt);          // fires DUE timers — gesture deadline Timers included
flushMicrotasks();
handleBeginFrame(now);         // transient callbacks: animation tickers read `now`
flushMicrotasks();
handleDrawFrame();             // persistent callbacks: build -> layout -> paint -> semantics
```

The mapping and the rationale:

- **Clock first.** Both Flutter timers and FLUI deadlines/controllers must observe the new
  instant before anything reads it. `elapse(dt)` ≡ `clock.advance(dt)`.
- **Gesture deadlines before the frame.** Flutter fires deadline `Timer`s inside `elapse`,
  *before* `handleBeginFrame`. We mirror that: `poll_deadlines()` runs before the build.
  This is a deliberate divergence from the order the prompt lists (animations, then
  deadlines): a deadline callback may `controller.forward()`, and firing deadlines first
  means the controller's run starts at the same instant Flutter would, then ticks this
  frame at elapsed≈0 (no visible movement) — exactly Flutter's behavior. The reverse order
  is *observationally equivalent for the common case* (both land in the inbox before
  `build_scope`); the only case it matters is a deadline→`forward()` chain, where
  deadline-first is the faithful order. **Either order keeps the invariant that everything
  that can dirty the tree runs before `build_scope` drains the inbox** — that invariant is
  the real contract; the intra-step order is the tie-break, resolved toward Flutter.
- **Animation tick maps to `handleBeginFrame`'s transient phase.** We call `tick_at`
  directly (not via `handle_begin_frame`) because the auto-ticker is wall-clock (§0). The
  *effect* — controllers advance to the new instant and mark dependents dirty — matches
  Flutter's transient-callback phase.
- **`build_scope` + `run_frame` map to `handleDrawFrame`** (persistent callbacks: build,
  then layout/paint). FLUI has no microtask queue between phases that matters here; the
  inbox drain at `build_scope` start subsumes Flutter's `flushMicrotasks` before build.

No `handle_begin_frame`/`handle_draw_frame` Scheduler call is on this path — the Scheduler's
frame lifecycle is for the *production* wall-clock loop. Keeping the headless binding off
it is what makes the frame deterministic and parallel-safe.

---

## 3. Polling a per-detector arena → the shared/global arena decision

**Problem (verified).** `GestureDetectorState.arena` is a private `GestureArena::new()`
(`crates/flui-widgets/src/interaction/gesture_detector.rs:132,157`) with no accessor. The
detector drives `arena.close(pointer)` itself on pointer-down (`:252`) but **never calls
`poll_deadlines`** — it has no frame callback. `pump_frame` cannot reach that arena, and the
singleton `GestureBinding`'s arena is a *different* instance, so
`GestureBinding::tick_deadlines()` would poll the wrong arena. The detector's own doc admits
this: it works "at the cost of NOT competing with overlapping detectors (that needs the
binding's shared arena; a future enhancement)" (`gesture_detector.rs:130`).

**Decision: this motivates the documented "global GestureBinding arena."** A per-detector
arena that nobody polls is a structural dead-end for *every* deadline-driven recognizer
(long-press, and the press-delay of double-tap). The fix is to make the arena a **shared**
object the detector obtains rather than constructs:

- The detector acquires its `GestureArena` from an ambient source (a binding/context) instead
  of `GestureArena::new()`. The shared arena carries the clock (§1) and is polled once per
  frame by `pump_frame` (production: by the real binding's per-frame `tick_deadlines`).
- This also fixes the documented cross-detector competition gap for free (overlapping
  detectors share one arena, so they actually compete — Flutter parity).

**Scope split (so the minimal binding does not require the full GestureDetector rewire):**

- **Minimal (Phase 2):** the long-press *test* (§5) constructs a `LongPressGestureRecognizer`
  directly against `binding.arena()` (the shared, clock-bound arena), `add_pointer`s, and
  pumps frames. This proves the clock + poll route **without** touching `GestureDetector`
  (which does not even wire long-press yet). It mirrors the existing
  `held_pointer_fires_long_press_via_arena_poll` test, with `pump_frame` replacing
  `thread::sleep`.
- **Follow-on (deferred, tracked as the "global arena" item):** route `GestureDetector` (and
  a future `LongPressGestureDetector`) through the shared arena, so production detectors are
  polled by the real binding and overlapping detectors compete. This is a `flui-widgets` +
  ambient-arena-plumbing change, orthogonal to the headless binding, and is where the
  GestureDetector private-arena note gets retired.

> Interim alternative considered and rejected for the minimal scope: have `GestureDetector`
> register a per-frame `poll_deadline` callback at mount (keeping per-detector arenas). It
> keeps the dead-end shape (N arenas, no cross-detector competition) and adds a second
> frame-callback channel. The shared arena is the correct direction and is *not more work*
> for the minimal test — so we point at it now and take the small step toward it.

---

## 4. Widget-owned `AnimationController` → vsync / `TickerProvider` (scoped)

**Gap (verified).** flui-view has **no** `TickerProvider`/vsync. `AnimationController::new`
takes an `Arc<Scheduler>` and builds an auto-ticker (`controller.rs:242`), but that ticker
is wall-clock and ignores the injected vsync time (§0). So for *implicit* animations
(`AnimatedContainer`/`AnimatedOpacity`), where the widget owns the controller and the test
only calls `pump_frame(dt)`, the binding must advance the controller deterministically
itself.

**Decision (minimal): explicit controller registration + direct `tick_at`.**

- `AnimationController` is `Clone` (Arc-backed, `controller.rs:99`). The binding holds
  `controllers: Vec<AnimationController>`; a widget/test registers its controller via
  `binding.register_controller(c.clone())`.
- `pump_frame` advances each via `tick_at(elapsed)`, where `elapsed` is seconds on the
  virtual timeline **since that controller's current run started**. Because `tick_at`'s
  argument is "ticker-timeline seconds since start" and the controller re-zeros its run
  epoch on each `forward()`/`reset()` (`restart_ticker`, `controller.rs:1011`), the binding
  records the virtual instant at registration/run-start and passes `clock.now() − run_start`.
  A thin `RegisteredController { controller, run_start: Instant }` carries that, refreshed
  when the controller restarts. (Simplest correct form; avoids reaching into controller
  internals.)

**Deferred (follow-on, explicitly out of the minimal scope):** a real view-layer
`TickerProvider` so a `ViewState` acquires vsync transparently (`vsync: this`, Flutter's
`SingleTickerProviderStateMixin`). That requires **two** larger pieces:

1. A `TickerProvider` surfaced through the view layer / build context, wired to the binding,
   so `create_state` can mint a controller bound to the frame loop without the test knowing.
2. **Virtualizing the ticker/scheduler clock** so `Scheduler::handle_begin_frame(now)` is
   deterministic (the auto-ticker reads the injected `now`, not `Instant::now()` —
   `ticker.rs:667`). Only then can implicit animations be driven through the Scheduler's
   transient-callback phase instead of explicit `tick_at`.

Both are real cross-crate work (flui-view + flui-scheduler) and are the natural *next*
spec after this binding lands. The minimal binding proves the animation route with explicit
registration; it does not pretend to be the production vsync path.

---

## 5. Test strategy — deterministic long-press

The keystone red→green test. It must **fail on `main`** (no sleep ⇒ the wall-clock deadline
never elapses) and pass after the §1 clock change — proving the virtual clock drives the
deadline.

```rust
// crates/flui-interaction/tests/headless_long_press.rs  (or in the binding crate)
#[test]
fn long_press_fires_on_pumped_frames_without_wall_clock_sleep() {
    let clock = ManualClock::new();
    let arena = GestureArena::with_clock(Arc::new(clock.clone()));

    let fired = Arc::new(Mutex::new(false));
    let f = fired.clone();
    let recognizer = LongPressGestureRecognizer::with_settings(
        arena.clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
    ).with_on_long_press_start(move |_| *f.lock() = true);

    // Pointer down captures down_time from the VIRTUAL clock (now = base + 0).
    let pointer = PointerId::new(2).unwrap();
    recognizer.add_pointer(pointer, Offset::new(Pixels(10.0), Pixels(10.0)));

    // Hold still. Pump frames totalling < 500ms: must NOT fire.
    for _ in 0..3 { clock.advance(Duration::from_millis(100)); arena.poll_deadlines(); }
    assert!(!*fired.lock(), "must not fire before the deadline elapses");

    // One more frame crosses 500ms of VIRTUAL time: fires, deterministically.
    clock.advance(Duration::from_millis(200));
    arena.poll_deadlines();
    assert!(*fired.lock(), "held pointer past the deadline must fire on the pumped frame");
}
```

Then the same assertion at the `HeadlessBinding` level once it exists, going through
`binding.pump_frame(Duration::from_millis(200))` instead of `clock.advance` + `poll_deadlines`
directly — proving the *ordering* (deadline poll happens inside the frame, before
`build_scope`/`run_frame`). And a no-regression run of the existing `long_press.rs` tests,
which keep their `thread::sleep` against `SystemClock` (those exercise the production clock
path and must stay green).

Anti-cheating notes (per `AGENTS.md` Definition of Done):
- The test is sleep-free; if it passed without the §1 change it would be a tautology — it
  must be confirmed red on `main` first.
- Do not narrow the existing sleep-based tests; they cover the `SystemClock` path. Add, don't
  weaken.

---

## 6. Phased build plan (smallest first)

Each phase is independently shippable and gated (`just ci`: fmt-check → clippy → test;
port-check).

**Phase 0 — clock primitive (flui-interaction).** Smallest, highest-leverage, no orchestration.
- Add `crates/flui-interaction/src/clock.rs` (`MonotonicClock`, `SystemClock`, `ManualClock`);
  export from `lib.rs`.
- `GestureArena`: add `clock` field, `with_clock`, `now()`; `new()` defaults to `SystemClock`
  (behavior unchanged). `RecognizerBase`: add `now()` delegating to `arena.now()`.
- `long_press.rs`: replace the two `Instant::now()` reads (`:271`, `:423`) with `self.state.now()`.
- Test §5 (recognizer-direct, sleep-free) — **red on `main`, green here**.
- In-scope: clock + long-press source swap. Out: any binding type, any other recognizer.

**Phase 1 — `HeadlessBinding` skeleton + `pump_frame` ordering.**
- New `HeadlessBinding` (start in `flui-widgets/tests/common`, or the chosen crate per §2):
  owns `ManualClock`, a clock-bound shared `GestureArena`, `BuildOwner`, `ElementTree`,
  `Arc<RwLock<PipelineOwner>>`.
- `pump_frame(dt)` implementing the §2 order: `clock.advance` → `poll_deadlines` →
  (controllers, Phase 3) → `build_scope` → `run_frame`.
- Re-express `LaidOut::pump()`/`tick()` (`tests/common/mod.rs:156,175`) in terms of it (or
  delegate), so existing widget tests keep working unchanged.
- Test: long-press fires via `binding.pump_frame(...)` (proves in-frame ordering).

**Phase 2 — shared arena for `GestureDetector` (the "global arena" step).** *(May ship after
Phase 3; independent.)*
- Plumb an ambient/shared `GestureArena` to `GestureDetector::create_state` instead of
  `GestureArena::new()` (`gesture_detector.rs:157`). Retire the private-arena limitation note.
- Test: two overlapping detectors compete in one arena (Flutter parity), and a detector's
  deadline-driven recognizer is polled by `pump_frame`.

**Phase 3 — controller registry + implicit-animation route.**
- `HeadlessBinding::register_controller` + `RegisteredController { controller, run_start }`;
  `pump_frame` step 3 advances each via `tick_at(now − run_start)`.
- Test: a registered controller animates across pumped frames; an `AnimatedView`/`FadeTransition`
  over it shows the value change frame-to-frame (the FadeTransition follow-on from the
  animation-scheduling spec).

**Deferred (separate spec, not this binding):**
- View-layer `TickerProvider` (`vsync: this`) + virtualized ticker/scheduler clock, so implicit
  animations run through `handle_begin_frame(now)` rather than explicit registration (§4).
- `DoubleTapGestureRecognizer` deadline made clock-driven (same one-line source swap as
  long-press, once it reads `self.state.now()`); double-tap press-delay then deterministic.
- Production wiring of `pump`-equivalent ordering into the real `AppBinding` draw loop
  (calling `tick_deadlines` + `flush_pending_moves` per frame against the shared arena).

---

## ARCH-GATE

- [x] **Boundaries / dependency direction acyclic.** Clock lands in `flui-interaction` (a
      leaf the arena already owns). The binding sits at/above `flui-widgets`, depending
      downward only. No cycle.
- [x] **No layering violation.** The headless binding is test/embedder infrastructure; it does
      not push domain state into a lower layer. The clock is read by the layer that owns the
      deadline (interaction), not injected from above per-call.
- [x] **One fact, one place.** The clock is the single time source for deadline recognition;
      `SystemClock` is the one production reading of `Instant::now()` for that path. The
      long-press timeout still reads `GestureSettings::long_press_timeout()` (one home,
      `settings.rs:396`). The test reads the same `MonotonicClock` the code does.
- [x] **Reuse over reinvent.** `poll_deadlines`, `tick_at`, `build_scope`/`external_inbox`,
      `run_frame`, and `GestureBinding::tick_deadlines`/`flush_pending_moves` are reused
      verbatim; only the *time source* and an *orchestrator* are new.
- [x] **Public surface intentional / semver-aware.** New public items:
      `MonotonicClock`/`SystemClock`/`ManualClock` and `GestureArena::with_clock`/`now`
      (additive; `new()` preserved). `HeadlessBinding` is test-support surface. Consult
      `api-design-lead` before promoting `HeadlessBinding` out of test scope.
- [x] **Implementable by the owning leads as scoped.** Phase 0 = `cli-ux`/interaction-level
      source swap; Phase 1–3 = view/rendering/animation orchestration. No hidden cross-domain
      coupling beyond the clock primitive.
- [x] **Struct split / new crate justified.** No struct split. The only new-crate question is
      the binding's home (§2, `flui-binding` vs `flui-app testing`) — flagged for confirmation;
      Phase 0–1 do not block on it.
- [x] **Forward view (2-year / 3-extension).** The clock generalizes to double-tap, force-press,
      and any future deadline recognizer (read `self.state.now()`); the binding generalizes to a
      production embedder loop; the shared arena unlocks cross-detector competition. The
      deferred TickerProvider is the named next extension.
- [x] **Boundary types considered.** `MonotonicClock` is a small, sealed-by-convention trait
      (object-safe, `Arc<dyn>`-friendly) — the right shape over a concrete enum, because the
      production vs virtual split is exactly a behavior swap. `GestureSettings` stays
      `#[non_exhaustive]` (unchanged).

**Verdict: COMPLETE (design).** Recommendation: proceed Phase 0 → 1 → 3 (Phase 2 parallelizable).

### Delegation plan
- **chief-architect** (this doc) → confirm the §2 crate-placement fork (`flui-binding` new
  crate vs `flui-app` `testing` feature) with the user before Phase 1 promotes the binding out
  of `tests/common`.
- **Phase 0** → `cli-ux-lead` is not the owner; route the interaction-crate change to the
  gesture owner via `/dev-task` (clock + arena + long-press source swap + §5 red→green test).
- **Phase 1/3** → `async-systems-lead` is not the owner; the frame-orchestration + controller
  registry is view/rendering/animation work — route via `/dev-task` to `rust-builder` under the
  relevant lead, governed by this plan.
- Record this plan's durable decisions via `/adr` (clock-on-arena; shared-arena direction;
  explicit-controller-registration-now / TickerProvider-deferred).

MEMORY: Headless frame driver — DECISION clock-on-`GestureArena` (`MonotonicClock` trait +
`SystemClock` default/`ManualClock` virtual), NOT push-`now`-through-poll: localizes injection
to one ctor, leaves `poll_deadlines()`/`poll_deadline()` + `add_pointer` signatures unchanged,
mirrors `AnimationController`-holds-`Arc<Scheduler>`. Long-press obstacle = two `Instant::now()`
reads (`long_press.rs` handle_down:271 + try_fire_timer:423) → `self.state.now()`. `pump_frame`
order = advance clock → `poll_deadlines` (Flutter fires gesture Timers during `fakeAsync.elapse`
BEFORE frame, deadline-first beats prompt's animation-first) → controllers `tick_at` → `build_scope`
(drains `external_inbox`) → `run_frame`; invariant = everything that dirties the tree runs before
`build_scope`. Auto-ticker is wall-clock (`ticker.rs:667` ignores injected vsync) so controllers
MUST be `tick_at`-driven, not via `handle_begin_frame`. Per-detector `GestureArena::new()`
(`gesture_detector.rs:157`, private, never polled) is the dead-end motivating the shared/global
arena. `GestureBinding` already has `tick_deadlines()`/`flush_pending_moves()`. DEFERRED: view-layer
`TickerProvider` + virtualized ticker clock; GestureDetector shared-arena rewire; double-tap.
HeadlessBinding must be non-singleton (RenderingFlutterBinding is a singleton, CI `--test-threads=1`).
