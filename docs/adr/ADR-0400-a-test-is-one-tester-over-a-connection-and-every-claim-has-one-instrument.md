# ADR-0400: A test is one `Tester` over a connection, and every claim has one instrument

- **Status:** Proposed (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:** once accepted, [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) whole (§8 below
  restates the rules of it that stand); [ADR-0238](ADR-0238-flui-driver-protocol.md) §8 only for
  the driver's `headless` feature and its edge to `flui-testing`, and §9 only for the Rust front
  end's `Window`, `Element` and `expect` (the driver supplies a connection, §1)
- **Related:** [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (a test's frame is
  `UiRuntime::pump`); [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (the agent
  that answers the vocabulary); [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (rules are types); [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  (no source gate); [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  (the fixed gate a golden step joins, inside `test`);
  [ADR-0396](ADR-0396-a-perf-scenario-is-one-workload-every-instrument-measures-and-its-costs-are-a-file-beside-it.md)
  (the cost instrument, placed here and unchanged)
- **Sources:** `flutter/flutter@3.50.1` (commit `ae9b586f`) `packages/flutter_test`,
  `packages/integration_test`; `linebender/xilem@9a78df87` `masonry_testing`;
  `emilk/egui@ea6b0943` `crates/egui_kittest`; `rerun-io/kittest@11c1a3f1`;
  `zed-industries/zed@c506354e` `crates/gpui`; `androidx/frameworks/support@f555a473`
  `compose/ui/ui-test`; cited inline

## Context and problem statement

`flui-testing` grew one module for each question a test asked. Counted by name at `main`
`aa45aa8`:

| What | Count | Where |
|------|-------|-------|
| Types that mount a tree and drive it | 4 in the kit: `HeadlessHost` (122 public methods), `LaidOut` (113), `Harness` (37), `Tester` (73); and `flui_driver::Window`, `flui_app::testing::HeadlessRun`, `flui_rendering::testing::RenderTester` | `crates/flui-testing/src/{host.rs,widgets.rs,widgets/harness.rs,tester.rs}` |
| Scenario APIs an author can write | 2: `Tester` with `Expectation`, and the driver's `Window`, `ElementLocator` and `expect` with `Expect` and `SoftExpect` | `crates/flui-testing/src/tester.rs`, `crates/flui-driver/src/{window.rs,expect.rs}` |
| Finder families | 3: `find` (the protocol's `Locator`), `a11y::A11yQuery`, `LaidOut::find_by_render_type` and `find_text` | `crates/flui-testing/src/{find.rs,a11y.rs,widgets.rs}` |
| Input vocabularies | 4: `dispatch_pointer_*`, `replay::PointerScript` and its four siblings, `Tester`'s verbs, a recording's replay | the same files, `replay.rs`, `recording/` |
| Settle predicates | 2 whose rules differ: the host's reads six conditions the runtime's cannot see | `crates/flui-testing/src/host/settling.rs:186`, `crates/flui-runtime/src/ui_runtime/agent_wait.rs:214` |
| Protocol methods the runtime does not answer | 7: `AuditA11y`, `CapturePresented`, `CaptureRendered`, `PerfStart`, `PerfStop`, `WaitFrames`, `WaitSettled` | `crates/flui-runtime/src/ui_runtime/agent_serve.rs:385` |
| Test files on `lay_out*`, and on `Tester` | about 197, and 26 | `crates/*/tests`, `packages/*/tests` |
| Copies of a build-counting `Probe` view; hand-written `pump`, `advance`, `tick` or `settle` in test code | 20; 80 | the same trees |

The form `AGENTS.md` gives an application author (`Tester`) is not the form the framework's own
widgets are tested on (`LaidOut`), and a scenario written for the headless host is written again
for a device, against the driver's types. Nothing renders a widget's pixels below the engine's
readback suite, no check reads an accessibility rule in one line, each of the 32 `proptest!`
blocks writes its own generators, and a settle that does not end reports a frame count
(`crates/flui-testing/src/tester.rs:574`), which is the bare `pumpAndSettle timed out` of
`flutter/flutter@3.50.1 packages/flutter_test/lib/src/widget_tester.dart:717`.

Which types does a test use at each depth, which vocabulary does it find, act and assert with, and
which instrument proves each kind of claim, for the framework's tests and an application's alike?

## Decision drivers

- Flutter's `flutter_test` is the source of the author-facing shape: one `WidgetTester` over
  sibling bindings, on a device too (`binding.dart:1070,2295,2800`,
  `packages/integration_test/lib/integration_test.dart:43`). A departure names a Rust reason or a
  defect Flutter's tracker records.
- One form per need (`AGENTS.md`, "Current forms"): one harness at a depth, one finder, one input
  and one matcher vocabulary, and each replaced form is deleted.
- A scenario is written once and runs headless on a virtual clock, on a device through
  `flui-driver`, and from an agent.
- A test proves what the product does: its frames are `UiRuntime::pump`, its input takes the
  runtime's input path, a double sits at a capability seam, time is virtual, nothing is
  process-global.
- Rules are types (ADR-0303): acting on an ambiguous locator, reading before a frame and a
  frame-exact check before settling should not compile.
- A failure is a diagnosis an engineer or an agent acts on without a rerun.
- Tests are cheap at this scale (4,833 tests): no new crate, one integration binary a crate, the
  kit's default build smaller than today's.
- Tooling is part of the platform: what measures FLUI is what an author uses on a screen of their
  own, through `flui::testing` and `flui test`.

## Considered options

1. Keep the harnesses per depth and hold the rules by review (ADR-0243 as it stands).
2. Port `flutter_test`: a binding hierarchy, a `WidgetTester` with by-type and by-key finders
   over the element tree, a matcher evaluator in the test process.
3. Make the driver's handle-chaining API the one scenario API (`window.locate(..).tap()?`,
   `expect(..)`), and move the framework's tests onto it.
4. One `Tester` over a connection, with the agent protocol as the one vocabulary and the frame's
   internals readable only in process.
5. One harness type per depth behind a shared trait of verbs.

## Decision

Chosen option: 4, because it is the only one where a scenario's finder, input and assertion are
evaluated once, in the application's runtime, on every target, and where the depths differ by a
type parameter instead of by a second API.

### §1. Two harness types, and a depth is a connection

`flui_rendering::testing::RenderTester` is the render-object depth. `flui_testing::Tester<C>` is
every depth above it. A depth supplies a connection, not a harness:

```rust
// flui_protocol::client
pub trait Connection {
    /// One request to the window's agent. Never runs a frame and never moves the clock.
    fn call<M: WindowMethod>(&mut self, params: M::Params) -> Result<M::Output, CallError>;
    /// Hold out for `condition`; the application's evaluator answers (§3).
    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError>;
    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError>;
    fn advance(&mut self, by: Duration) -> Result<Advanced, CallError>;
    fn now(&self) -> At; // frame number, time, and whether the clock is virtual
}

// flui_testing
pub struct Tester<C = Headless> { /* connection, window, journal, teardown policy */ }
pub struct Headless<S = Observed> { /* the headless host, private */ }
pub struct Observed { /* the window's agent, its input lease */ }
pub struct Unobserved; // no agent was handed the window: the state a cost baseline holds
```

| Connection | Depth | Flutter counterpart |
|------------|-------|---------------------|
| `Headless<Observed>` | a view mounted as `run_app` mounts it, on a manual clock | `AutomatedTestWidgetsFlutterBinding` |
| `Headless<Unobserved>` | the same host with no agent attached, for cost scenarios (§5) | none: Flutter's benchmarks are a separate harness |
| `flui_app::testing::AppRun` | a whole `Application` on the headless platform | none |
| `flui_driver::Device` | a running application on a real target | `IntegrationTestWidgetsFlutterBinding` |

`Headless<Unobserved>` is not a `Connection`: with no agent nothing resolves a locator, so it has
the in-process readings of §4 and no scenario verbs. `HeadlessHost` becomes private to the kit.
`LaidOut`, `Harness`, the driver's `Window`, `ElementLocator`, `Element`, `Expect`, `SoftExpect`
and in-process lane, and `flui-cli`'s own agent client are deleted.

An author writes what Flutter's author writes, without `await`: the frame path is synchronous.

```rust
let mut t = Tester::mount(counter());
t.tap(&find::button("Increment"));
t.expect(&find::id("count"), value("1"));
```

A scenario for every target is a function over the connection, and `scenarios!` lists it once:
the macro makes a headless `#[test]` of each and a `pub const SCENARIOS` the crate's `device`
target runs through `Tester::attach(Device::launch(..)?, window)`.

```rust
fn increments<C: Connection>(t: &mut Tester<C>) { /* … */ }
flui::testing::scenarios! { increments }
```

### §2. The vocabulary is the agent protocol, evaluated in the application

A finder is `Find<Q>` over the protocol's `Locator`; `Q` is `One` or `Many`.

```rust
pub struct Find<Q = One> { /* Locator */ }
impl Find<One>  { pub fn within<Q>(self, ancestor: Find<Q>) -> Self;
                  pub fn has<Q>(self, descendant: Find<Q>) -> Self;
                  pub fn all(self) -> Find<Many>; }
impl Find<Many> { pub fn first(self) -> Find<One>; pub fn nth(self, index: u32) -> Find<One>;
                  pub fn last(self) -> Find<One>; }
pub mod find { /* text, text_containing, button, role, id, with_state, root: each a Find<One> */ }
```

An action takes `&Find<One>`, so acting on a set does not compile; one that matches two elements
at run time is `TestError::Ambiguous` with every candidate, never the first
(`flutter_test` warns and acts, `controller.dart:1070`, flutter/flutter#100758). A finder is a
description resolved again at each use, never a node handle (Compose's `SemanticsNodeInteraction`).
There is no finder by widget type, by render type or by a kind string: a scenario that names a
type does not survive the move to a device or a refactor of the widget.

A matcher is data that lowers to exactly one `flui_protocol::wait::Condition`:
`exists`, `gone`, `count(n)`, `name`, `value`, `value_contains`, `description`, `state`,
`checked`, `enabled`, `disabled`, `focused`, `visible`, `all([..])`, `any([..])`. The protocol's
`Expectation` gains `Visible` and `Description` for them and no other type is added. The
application's runtime evaluates the condition at frame boundaries and after input (ADR-0238 §7);
the test process has no second evaluator, so `t.expect(&find, matcher)` waits the same way on
every connection. A claim the wire cannot carry (a rectangle, a negation, a typed field) is read
(`rect`, `read`, §4) and asserted in Rust, which does not wait.

Input is `flui_protocol::input::ActionSequence`. `Tester`'s verbs (`tap`, `long_press`, `drag`,
`fling`, `scroll_into_view`, `enter_text`, `press_keys`) and `Gesture`, the builder for
multi-pointer and timed input, both produce it; one function, `flui_protocol::input::plan`, turns a
sequence into timed input for the agent's injection and for a platform lane alike. Before pointer
input the runtime answers actionability, and a hidden, covered, disabled or detached target is
`TestError::NotActionable` with nothing sent. `dispatch_pointer_*` and `replay`'s five script
types are deleted; a recording is the serialised journal of these steps.

Every verb has a `try_` twin returning `TestError`; the plain form panics at its caller
(`#[track_caller]`). `TestError` carries a `Diagnosis`: the journal step and its caller, the
frame and the time, the nearest candidates, an outline excerpt, and for a wait the unsettled
causes of §3. `Tester::checks(|c| ..)` gathers soft failures and raises one report after the
closure.

### §3. Settledness has two owners behind one predicate

What keeps an application from being idle is owned partly by the runtime (a redraw, a scheduled
frame, pending work, a timer) and partly by its host (a held background job, a turn owed for a
close or a quit, a window observation not yet delivered, a timed frame request, a scenario step,
a frame a covered window owes). The host states its half through a seam the runtime reads:

```rust
// flui_runtime
pub trait HostWork {
    fn unsettled(&self, now: Instant, causes: &mut Vec<UnsettledCause>);
}
impl UiRuntime { pub fn unsettled(&self, now: Instant) -> Vec<UnsettledCause>; }
```

`UiRuntime::unsettled` is the runtime's causes and the host's. The headless host's
`pump_until_settled`, its `time_to_next_frame` and the evaluator of `Condition::Settled` call it
and nothing else decides idleness. `UnsettledReason` gains a variant for each host cause and for a
running ticker, and `UnsettledCause` gains the element that owns it and the source location that
created it, where the runtime records one. A settle that does not end fails with every cause
named, and a wait whose condition is false while the list is empty fails at once as `Idle`, so a
timer of the application never elapses inside an expectation.

A host that does not implement `HostWork` (a runner, until it does) answers the runtime's half
only, and the report of a `Device` scenario says so (`settle_scope: runtime`).

The runtime answers `wait.settled`, `wait.frames`, `audit.a11y`, `perf.start` and `perf.stop`,
which are `not_answered!` today; the two captures stay with a host that has a renderer.

### §4. A frame's internals are read in process, through types

`Tester<Headless<S>>` has what only an in-process host can give, so a cross-target scenario that
touches it does not compile:

```rust
impl<S> Tester<Headless<S>> {
    pub fn frame(&mut self) -> Frame<'_, S>;   // exclusive: nothing acts or pumps while it lives
    pub fn measure(&mut self, steps: impl FnOnce(&mut Self)) -> Cost;   // ADR-0396 §1
    pub fn clock(&self) -> &HostClock;
    pub fn work(&mut self) -> Work<'_>;        // hold and run background work
    pub fn double<D: TestDouble>(&self) -> &D::Double;
}
impl<S> Frame<'_, S> {
    pub fn only<R: RenderObject>(&mut self) -> Result<RenderGeometry, TestError>;
    pub fn render_all<R: RenderObject>(&mut self) -> Vec<RenderGeometry>;
    pub fn layers(&self) -> LayerOutline;
    pub fn draw_list(&self) -> DrawList;
    pub fn report(&self) -> &FrameReport;
}
impl Frame<'_, Observed> {
    pub fn geometry<R: RenderObject>(&mut self, at: &Find) -> Result<RenderGeometry, TestError>;
    pub fn read<R: RenderObject, T>(&mut self, at: &Find, read: fn(&R) -> T) -> Result<T, TestError>;
    pub fn draw_commands(&mut self, at: &Find) -> Result<DrawList, TestError>;
}
```

A typed read selects render objects by `TypeId` (`downcast_ref::<R>`) at or under the element a
`Find` names, requires exactly one, and copies an owned value out inside the pipeline's
closure-scoped access; no borrow of a render object leaves the frame. `Frame::read` replaces
`LaidOut`'s one-off probes (`opacity_paint_alpha`, `clip_rrect_border_radius`,
`fitted_box_transform`, `image_paint_rect`, …) and the name-based `find_by_render_type`; its
counterpart is `tester.renderObject<T>` (`controller.dart:987`). No `Tester` exists before its
first presented frame, so a read before a frame cannot be written. A test that needs a loose or
tight constraint mounts a visible `fixture::Constrain` view; the harness wraps nothing around the
root that the test did not write.

A read is one owner turn with no frame (`UiRuntime::drain_owner_inbox`): it applies commands
queued since the last turn and presents nothing. Geometry read through one `Frame` is that of one
presented frame.

### §5. Every claim has one instrument

| Claim | Instrument | Flutter 3.50.1 counterpart, or the reason to depart |
|-------|------------|------------------------------------------------------|
| Behaviour | `Tester::expect(&find, matcher)` | `expect(finder, matcher)`, `matchers.dart:46`; the matcher is wire data with one evaluator |
| Geometry | `Tester::rect` on every connection, `Frame::geometry::<R>` in process | `getRect`, `rectMoreOrLessEquals`; tolerance is `LogicalRect::approx_eq` |
| Paint | `Frame::draw_commands -> DrawList`; `FrameRun::paint()` at the render-object depth | `paints`, `mock_canvas.dart:54`; no pattern language: Rust over an owned list |
| A render object's field | `Frame::read::<R, T>` | `renderObject<T>`, `controller.dart:987`; by `TypeId`, never a name, never on the wire |
| Pixels | `Settled::golden` (§6) | `matchesGoldenFile`, `matchers.dart:589` |
| Semantics | `Tester::read`, `Tester::semantics` | `matchesSemantics`, `matchers.dart:666` |
| Accessibility rules | `Settled::audit(&[AuditRule])` | `meetsGuideline`, `matchers.dart:1301`; contrast from the draw commands' colours and target size from the hit region inside its clip, not from pixels (flutter/flutter#103235, #144121); a rule that cannot run is `cannot_verify`, never a pass |
| Time and animation | the virtual `HostClock`; `pump`, `pump_for`, `settle` | `pump`, `pumpAndSettle`, `widget_tester.dart:651`; a cause is named (#114063) and there are no zones |
| Background work | `UnsettledReason::Background`; `work().hold()` and `run()`; a seed orders ready completions | `runAsync`; gpui's seeded scheduler |
| Platform capabilities | `double::<D>()`: the double of the crate that owns the seam | `TestDefaultBinaryMessenger`; typed |
| Ownership and leaks | `DropLedger`, `Recorder<V>`, the teardown checks of §7 | `binding.dart:2638` |
| Panics and containment | `try_*` returning `TestError::FrameFailed`; one channel | `takeException`; two channels there (#151965) |
| Cost | `Tester::measure` and `perf::Baselines` (ADR-0396); `Tester::trace` on a device | `traceAction`, `integration_test.dart:350` |
| Robustness | `<crate>::testing::strategy`; `model::Model` over `Tester<C>`; the same model as a fuzz body; mutation score (`gamma.toml`) | none |

`Recorder<V>` wraps a view and records its lifecycle calls (builds, state creations, disposals);
it replaces `ProbeRoot`, `StateCreations` and the copied `Probe` views
(masonry's `Recorder`, `masonry_testing/src/recorder_widget.rs`).
A `strategy` module lives in the kit of the layer that owns the generated type (constraints and
sizes in `flui_rendering::testing`, geometry in `flui_foundation::testing`), because a unit test
below `flui-testing` cannot reach it (§8). A `Model` is a state machine of scenario steps with a
reference model; a failure prints the seed that reproduces it.

### §6. A golden is the product renderer's frame on one canonical adapter

`Tester::settle` returns `Settled<'_, C>`, the only receiver of `golden` and `audit`, so a
frame-exact check of a frame that is still moving cannot be written.

A headless golden renders the settled frame's layer tree with the engine's own
`HeadlessRenderer`, on the software adapter the pull-request job already installs for the
readback suites, and compares exactly with a PNG beside the test. On any other adapter the
outcome is `CannotVerify`, which fails `--goldens` runs that require it and is never a pass.
There is no second, CPU renderer: it would certify its own pixels instead of FLUI's
(masonry's deterministic pixels come from having one renderer, `masonry_testing/src/harness.rs:564`).
There is no default tolerance and no per-OS baseline tree (egui's thresholds per OS,
`crates/egui_kittest/src/snapshot.rs`; flutter/flutter#131559, #184182). A golden on a `Device`
states its tolerance and is keyed by backend and OS.

Goldens are a thin tier. A paint claim is an assertion over `DrawList` first (§5), and a crate
keeps few, small images; a text golden (an outline of a tree or a scene) is held by the same
`Goldens` holder and blessed the same way. `FLUI_BLESS=1` rewrites, and a run that failed writes
nothing. A crate's goldens are one `golden` test target that runs one test at a time.

### §7. A test's end is checked

`Tester::finish` and `Drop` run, in this order: gather soft failures; audit the last frame
(`AuditRule::Unnamed` by default); unmount; run drops; release the agent and the doubles; check
the `DropLedger`; check that §3's predicate is empty (a timer, a ticker or background work left
behind); raise one `TestReport`. Drops run before the checks (the order flutter/flutter#144472
asks for and `binding.dart:2638` does not have), and the harness's own objects are released
before the ledger is read (#169119). A verb that unwound poisons the `Tester`, and `Drop` checks
nothing when the thread is panicking or the `Tester` is poisoned: `thread::panicking()` is false
after a caught panic. `finish_unchecked(reason)` and `Audit::none().because(reason)` are the
stated exceptions.

### §8. The kit's layout, the facade, and the layer kits

`flui-testing` is laid out by claim: `scenario` (the `Tester`, its connections, `find`, the
matchers, `gesture`, the diagnosis and teardown), `frame`, `audit`, `perf`, `doubles`,
`ownership`, `fixture`, and behind features `golden`, `model` and `conformance` (the suites a
backend or a store author runs, out of the default build and out of the facade). `flui::testing`
re-exports all but `conformance`, with `rendering::RenderTester`. No crate is added and no
proc-macro.

The rules of ADR-0243 that stand:

- A crate exposes its test support behind a Cargo feature named `testing`, and the feature
  exposes the kit and nothing else.
- Each layer has one kit, `<crate>::testing`, built on the kit below; from the frame layer up it
  is `flui-testing`. A crate below it uses it from `tests/` through a dev-dependency and never
  from a unit test in `src/`, which is built against a second copy of the crate.
- A test takes its clock and its frames from its layer's kit.
- A double for a contract lives in the kit of the crate that owns the contract.

What changes: the kit does not grow a finder or a wait beside the driver's (ADR-0243 D5) because
the driver has none of its own (§1); and the allowlist of exceptions (D6, D7) is not rebuilt.
During the migration each old form is `#[deprecated]`, and a test module still on one carries
`#[expect(deprecated, reason = "docs/gaps.md#one-tester")]` on its `mod` line: a new module on an
old form fails `clippy`, and a migrated module whose attribute stays fails as an unfulfilled
expectation.

### §9. What is not built

A binding hierarchy or a global binding; a fake-async zone, `runAsync` or an `async` test body; a
wall-clock wait in a gate; a coordinate tap that warns; a finder by type or by kind string; a
pattern language for paint; a CPU renderer for goldens; a tolerance that defaults loose;
pixel-sampled contrast; `takeException` or a global assertion hook; a matcher evaluator in the
test process; a `#[flui::test]` proc-macro or a link-time test registry; a testing crate beside
`flui-testing`.

### Consequences

- Good, because a mounted tree has one harness above the render object, and the framework's
  widgets are tested on the form its authors are given.
- Good, because a scenario's verbs, finders and matchers exist once, and a device runs the
  function a headless test runs.
- Good, because a settle, a wait and an action fail with their cause, and a test that leaves a
  timer, a ticker or an owned value behind fails where it is written.
- Good, because the kit's default build loses the widget harness and the conformance suites.
- Bad, because about 330 test files move, 200 of them from direct pipeline reads to locates
  through the agent; mounting presents two frames where `lay_out` presented one.
- Bad, because a pixel golden is checked on one adapter: an author on macOS or Windows blesses
  and checks in the pinned image or in CI, and an update of the software rasteriser re-blesses
  every golden in one change.
- Bad, because a whole-`Application` run has no `Frame` and no `measure`: the runner keeps its
  runtime in thread-local state, so a cost scenario mounts the root view on `Headless`.
- Bad, because the wire has no negation and no rectangle condition, so those claims do not wait.
- Neutral, because cost scenarios run `Unobserved`, where input is aimed at a point taken from a
  typed read: the agent's journal and its work after each frame are not in the measured state.

### Confirmation

- The compiler: `Tester::tap(&Find<Many>)`, `Tester<Device>::frame()` and `golden` on anything
  but `Settled` are compile-fail fixtures of `flui-testing`.
- clippy: `deprecated` on the old forms with the `#[expect]` marker of §8, until the last one is
  deleted.
- Tests of `flui-runtime`: a table with one row per `UnsettledReason` in which
  `pump_until_settled` and `wait(Settled)` both stay unsettled and name the reason; one
  served-answer test per method removed from `not_answered!`.
- Tests of `flui-testing`: a read leaves the frame number unchanged; a caught panic followed by
  `Drop` runs no teardown check; `measure` on `Headless<Unobserved>` equals
  `LaidOut::pump_report` for the checked-in perf files until the latter is deleted; a golden off
  the canonical adapter is `CannotVerify`; a mismatch writes a diff and a failed run blesses
  nothing.
- A test of `flui-protocol`: one `ActionSequence` per gesture kind, planned once, gives the same
  routed events through the agent's injection and through a platform lowering.
- `cargo tree -p flui-cli -e normal` names no `flui-testing`, and `flui-driver` has no
  `headless` feature.

## Pros and cons of the options

### 1. Keep the harnesses per depth and hold the rules by review

- Good, because no test file moves.
- Bad, because it is the state that drifted: four harnesses and three finder families grew under
  ADR-0243 once its gate went (ADR-0371), and Compose (`runComposeUiTest` beside its `v2`) and
  gpui (`TestAppContext` beside `TestApp`) show a replaced test API kept alive stays alive.
- Bad, because the framework is proven on a form its authors are told not to use.

### 2. Port `flutter_test`

- Good, because an author who knows Flutter knows it.
- Bad, because its binding is process-global state, its fake-async zone exists for a frame path
  FLUI does not have, and a real future hangs it silently (flutter/flutter#5728).
- Bad, because finders by type and by key over the element tree do not reach a device or an
  agent, which is why Flutter has a second API there (`flutter_driver`, flutter/flutter#148028).
- Bad, because its tap warns and acts on a covered target (#100758) and its settle names no
  cause (#114063).

### 3. The driver's handle-chaining API as the one API

- Good, because the driver has it and an agent's tools mirror it.
- Bad, because every step returns `Result` and reads further from Flutter's `tester.tap(finder)`
  than any Rust reason requires.
- Bad, because the in-process readings (a render object's geometry, a cost window) have no place
  on a `Window` of a remote target, so the framework's tests would keep a second harness.

### 4. One `Tester` over a connection

- Good, because the verbs exist once and the depths differ by a type parameter.
- Good, because the vocabulary already exists and is the one an agent speaks.
- Bad, because of the migration and the two frames of a mount (Consequences).

### 5. One harness type per depth behind a shared trait

- Good, because each depth keeps a small type.
- Bad, because a scenario generic over the trait still has four implementations of every verb,
  and a matcher evaluated by each of them is four evaluators.

## More information

- The migration order, the measured experiments that precede its fan-out and the counts each
  step lowers are in `docs/plans/specs/testing-platform/`.
- Settle with a diagnosis: `emilk/egui@ea6b0943 crates/egui_kittest/src/lib.rs:476`
  (`ExceededMaxStepsError` with the repaint causes). Strict queries that print both matches:
  `rerun-io/kittest@11c1a3f1 src/query.rs:67`. An audit as part of settling:
  `crates/egui_kittest/src/accessibility.rs`. A no-op audit to avoid:
  `androidx/frameworks/support@f555a473 compose/ui/ui-test/src/commonMain/kotlin/androidx/compose/ui/test/Actions.kt:889`.
- A seeded order of ready work: `zed-industries/zed@c506354e crates/gpui/src/platform/test/dispatcher.rs:23`.
- Revisit §6 when the engine has a CPU backend of its own: the canonical adapter becomes that
  backend and a golden checks on every host. Revisit `Headless<Unobserved>` if an attached, idle
  agent measures no difference in any counter.
