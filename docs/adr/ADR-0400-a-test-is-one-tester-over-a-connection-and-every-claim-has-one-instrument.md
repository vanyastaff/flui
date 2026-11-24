# ADR-0400: A test is one `Tester` over a connection, and every claim has one instrument

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes in part:**
  [ADR-0243](ADR-0243-one-testing-kit-per-layer.md): D2's list of the kit's harnesses, D5 and D6
  (§8; D1, D3, D4 and D7's naming rules stand);
  [ADR-0238](ADR-0238-flui-driver-protocol.md): §5's last sentence and §6's "the driver repeats"
  for the synthetic and semantic paths (§2), §8's `headless` feature and its edge to
  `flui-testing` (§1), and §9's Rust front end, `Driver`, `Window`, `Element` and `expect` (§1);
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) §6, the script builders (§2);
  [ADR-0216](ADR-0216-gesture-alternatives.md): the gesture path of `assert_gesture_alternatives`
  driven through `PointerScript` (§2)
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (the tiers the golden
  seam of §6 keeps); [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (a test's frame is
  `UiRuntime::pump`); [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §4 (an
  act's reply deadline counts what the window holds it for, §2); [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) and
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §1 (what the contract crate may hold);
  [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (the agent that answers the
  vocabulary); [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (rules are types); [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  (no source gate); [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  (the fixed gate a golden step joins, inside `test`);
  [ADR-0396](ADR-0396-a-perf-scenario-is-one-workload-every-instrument-measures-and-its-costs-are-a-file-beside-it.md)
  (the cost instrument: `Tester::measure` calls its `perf::measure`, which is built with it)
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
| Test files on `lay_out*`, and on `Tester` | about 197, and 16 | `crates/*/tests`, `packages/*/tests` |
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
  frame-exact check of a moving frame should not compile.
- One owner for each piece of state: settledness, a test's failures and the input held between
  steps each have one, and nothing copies them.
- A failure is a diagnosis an engineer or an agent acts on without a rerun.
- Tests are cheap at this scale (4,833 tests): no new crate, one integration binary for each kind
  of target a crate has, the kit's default build smaller than today's.
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
5. One harness type per depth behind a shared trait of verbs with default bodies.

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
    /// Hold out until the application answers `condition` (§3), at most `limit` of the
    /// connection's clock.
    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError>;
    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError>;
    fn now(&self) -> ClockReading; // frame number, time, and whether the clock is virtual
    /// The end of a test: what this connection can check and release (§7).
    fn finish(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck>;
}

// flui_testing
/// A target whose clock the test moves; not a `Connection` bound.
pub trait VirtualClock {
    fn advance(&mut self, by: Duration) -> Result<Advanced, CallError>;
}
pub struct Tester<C = Headless> { /* connection, window, report */ }
impl<C> Tester<C> { pub fn connection_mut(&mut self) -> &mut C; }
pub struct Headless<S = Observed> { /* the headless host, private */ }
pub struct Observed { /* the window's agent */ }
pub struct Unobserved; // no agent was handed the window: the state a cost baseline holds
```

| Connection | Depth | `VirtualClock` | Flutter counterpart |
|------------|-------|----------------|---------------------|
| `Headless<Observed>` | a view mounted as `run_app` mounts it | yes | `AutomatedTestWidgetsFlutterBinding` |
| `Headless<Unobserved>` | the same host with no agent attached, for cost scenarios (§4) | yes | none: Flutter's benchmarks are a separate harness |
| `flui_app::testing::AppRun` | a whole `Application` on the headless platform | yes | none |
| `flui_driver::Device` | a running application on a real target | no | `IntegrationTestWidgetsFlutterBinding` |

`Connection` lives in `flui-protocol` because it is the one crate both the kit and the driver
reach. It names protocol types and `std` alone, so it is schema in ADR-0095 §1's sense, a
contract with no behaviour of a runtime behind it, and no upstream type enters the Stable crate's
surface (ADR-0089 §2). `CallError::Unanswered { may_have_run, released }` is a call whose answer
did not arrive; the connection asks the agent to release the lease's input and to cancel what it
holds, and `released` says whether the agent answered that it did, never assumes it.

`Headless<Unobserved>` is not a `Connection`: with no agent nothing resolves a locator (§4). It is
a `VirtualClock`, as are `Headless<Observed>` and `AppRun`; `Device` is not.
`HeadlessHost` becomes private to the kit. `LaidOut`, `Harness`, the driver's `Driver`, `Window`,
`ElementLocator`, `Element`, `Expect`, `SoftExpect`, `compose.rs` and in-process lane, and
`flui-cli`'s own agent client are deleted. `Device` is what `Driver` was: one launched target, its
lanes and its connection; `flui drive`, `flui mcp` and `flui-cli`'s other clients use it.

An author writes what Flutter's author writes, without `await`: the frame path is synchronous.

```rust
let mut t = Tester::mount(counter());
t.tap(&find::button("Increment"));
t.expect(&find::id("count"), value("1"));
```

A scenario for every target is a function over the connection, and `scenarios!` lists it once
with the root it mounts and its window. The macro makes a headless `#[test]` of each and a
generic `pub fn each<C: Connection>(visit: impl FnMut(&'static str, fn(&mut Tester<C>)))`, which
a crate's `device` target monomorphizes for `Device`. That target takes `flui-driver` as a
dev-dependency, which ADR-0238 §8 allows, and attaches through
`Tester::attach(Device::launch(..)?, spec)`; the facade has no `device` feature.

```rust
fn increments<C: Connection>(t: &mut Tester<C>) { /* … */ }
flui::testing::scenarios! { root: counter, window: WindowSpec::phone(), [increments] }
```

`attach` takes a `WindowSpec` (size, platform convention, scale, safe area). A headless
connection builds the window to it; a `Device` reads the window's `Placement`, which gains the
platform convention and the safe area, and refuses another with `TestError::WindowMismatch`
(ADR-0238 §11: a scenario fixes its window). What only a real target has (`perform_via` on the
`os` path, `os_tree`, `release_input`, pixel predicates with their stated tolerance) is a method
of `Device` itself, reached through `Tester::connection_mut`, so no crate needs an edge it lacks;
a capability a lane lacks is `TestError::NotSupported`.

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
type does not survive the move to a device or a refactor of the widget. A node that semantics
merged into an ancestor or never published is reached through §4's typed reads, which select by
`TypeId` at or under the element a `Find` names.

A matcher is data that lowers to exactly one `flui_protocol::wait::Condition`:
`exists`, `gone`, `count(n)`, `name`, `value`, `value_contains`, `description`, `state`,
`checked`, `enabled`, `disabled`, `focused`, `visible`, `all([..])`, `any([..])`. The protocol's
`Expectation` gains `Visible` and `Description` for them and no other type is added. The
application's runtime evaluates the condition at frame boundaries and after input (ADR-0238 §7);
the test process has no second evaluator, so `t.expect(&find, matcher)` is the same wait on every
connection, and §3 states how long it holds out. It holds once the condition is first true
(ADR-0238 §7), where Flutter's `expect` checks the frame the test pumped to; a claim about the
final state is written `settle` first, then the expectation or a read. A claim the wire cannot
carry (a rectangle, a negation, a typed field) is read (`rect`, `read`, §4) and asserted in Rust,
which does not wait.

Input is `flui_protocol::input::ActionSequence`. `Tester`'s verbs (`tap`, `long_press`, `drag`,
`fling`, `scroll_into_view`, `scroll_until`, `enter_text`, `press_keys`) and `Gesture`, the
builder for multi-pointer, keyed and timed input, both produce it; one function,
`flui_protocol::input::plan`, turns a sequence into timed input for the agent's injection, for a
platform lane and for the `HostInput` rows a conformance table replays. `Gesture` builds the
per-device sequences ADR-0215 §6's script builders built (touch, mouse, pen, trackpad and a
virtual pointer, which `InputSource` gains) and `Gesture::try_build` refuses an unpaired down or
up; `assert_gesture_alternatives` (ADR-0216) drives a gesture through it.

A gesture on the synthetic or semantic path is answered by the agent in the application: it
locates, checks actionability at each frame boundary until the target is attached, visible,
stable over two frames, enabled and receives the hit, or the action's timeout passes
(ADR-0238 §6), and injects in the turn the check passed, aimed at that frame's bounds. A failure
is `TestError::NotActionable` with the failed check and nothing sent. The action's timeout is in
its request: the conversation counts it as what the window may hold the act for (ADR-0288 §4),
the held act's deadline is a wake of §3, and when it passes the agent drops the act, so nothing
injects after its answer. `Tester::with(GestureOptions)` sets the protocol's options for the next
verb (`force`, which skips every check but attached, the timeout, the path). On the `os` path the
driver composes the gesture from those methods as ADR-0238 §5 says (the `os` half of today's
`compose.rs` stays), and its effect is `may_have_run` unless the application's input journal saw
the event reach the element. `scroll_until(&scrollable,
&find)` is answered the same way: the agent scrolls the scrollable a viewport at a time at frame
boundaries until the locator resolves or the scroll extent ends (`scrollUntilVisible`,
`controller.dart:2416`).

The input held between steps (keys down, pointers down) has one owner on each path: the window's
input lease in the agent for synthetic and semantic input, and the lane's own record for `os`
input. The `Tester` keeps no copy: a chord's keys that the path's owner already holds are not
pressed again, whichever step pressed them, and `Device::release_input` and the teardown release
both owners.

A recording stays ADR-0215's `HostInput` stream, replayed by `Tester<Headless<_>>::replay`. The
`Tester`'s journal (each step, its caller, its outcome) is the diagnosis written to the report,
not a second recording. `dispatch_pointer_*` and `replay`'s five script types are deleted.

Every verb has a `try_` twin returning `TestError`; the plain form panics at its caller
(`#[track_caller]`). `TestError` carries a `Diagnosis`: the journal step and its caller, the
frame and the time, the nearest candidates, an outline excerpt, and for a wait the unsettled
causes of §3. An acting verb ends with the owner turn that drains what its own frames queued, so a
failure a step caused is reported at that step and never at the read after it.

### §3. Settledness has one owner, the host that runs the turns

What keeps an application from being idle is partly the runtime's (a redraw, a scheduled frame,
pending work, its deadlines, a running ticker) and partly its host's (a held background job, a turn
owed for a close or a quit, a window observation not yet delivered, a timed frame request, a
scenario step, a frame a covered window owes its surface). The host that runs the runtime's turns
owns the list. After each turn, once its own steps after the pump are done (the owed frames
asked, the observations delivered), it gathers the runtime's half and its own into one
`Unsettled`, and that value is what its loop wakes for, what a settle reads, what the teardown
checks and what the agent's settled waits are answered from:

```rust
// flui_protocol::wait
pub struct UnsettledCause {
    pub reason: UnsettledReason,
    pub window: Option<WindowId>,     // none for a cause of the whole application
    pub owner: Option<ElementRef>,    // the element that owns it, where the runtime records one
    pub site: Option<SourceLocation>, // where it was created
    pub cleared_by: ClearedBy,
    pub due_in_ms: Option<u64>,       // for the report; the wake reads `Unsettled::wake`
}
pub enum ClearedBy {
    NextFrame,  // the next turn serves it
    Deadline,   // a turn at its deadline serves it: a timer, a frame's retry, a scenario step
    Test,       // only the test discharges it: held background work
    Outside,    // something outside the application answers it: a platform reply in flight
}
pub enum SettleScope { Complete, RuntimeOnly } // a field of `WaitOutcome` and of the report
pub enum WaitEnd { Satisfied, Unsatisfied, Idle } // how a wait ended: `WaitOutcome::end`

// flui_runtime
pub struct Unsettled { /* causes, and the exact `Instant` of the earliest wake */ }
impl Unsettled {
    /// The one wake rule every host calls: now for a `NextFrame` cause, the earliest deadline of
    /// a `Deadline` cause, a parked wait or a held act; never for `Test` or `Outside`.
    pub fn wake(&self) -> Option<Instant>;
}
impl UiRuntime {
    /// The runtime's half. A frame a surface owes is the host's half, never listed here.
    pub fn unsettled(&self, now: Instant, into: &mut Unsettled);
    /// The host, after its turn: answers the parked `Settled` and idle waits from both halves.
    pub fn answer_settled(&mut self, now: Instant, host: HostHalf<'_>);
}
pub enum HostHalf<'a> { Complete(&'a Unsettled), Absent }
```

The runtime's half derives each cause's `cleared_by` from its own state, so the combinations
the host reads today are the runtime's: a redraw while a frame's retry is armed is a `Deadline`
cause at the retry, and a redraw while the runtime's frames are off (no window seen) is not
listed. Every host (the headless host, `AppRun`'s runner, each native runner) wakes at
`Unsettled::wake` and keeps no rule of its own; `HeadlessHost::pump_until_settled` and
`time_to_next_frame` read it, and the runtime's `unsettled_causes` is deleted. A wake is an exact
`Instant` owned by whoever armed it; `due_in_ms` is its rounded copy for the report.

A wait holds out on the connection's clock. On a virtual clock the connection runs the host's loop,
moving the clock to `Unsettled::wake`, an application's timers included, until the runtime answers,
`limit` of virtual time has passed (the time a device would spend), or 10 000 turns have passed
without the clock moving, which ends the wait `WaitEnd::Unsatisfied` with the causes that
re-armed at the same instant (egui's `max_steps`). A wait whose condition is false ends
`WaitEnd::Idle` with the remaining causes when the host's half is `Complete` and every remaining
cause is `Test`: only the test can release it. An `Outside` cause keeps the wait parked to its
limit, as an `Absent` half does. A host whose half is `Absent` (a runner until it composes one) is
never idle, and its answers carry `SettleScope::RuntimeOnly`. `wait.settled` on a window counts
that window's causes and the causes of the whole application.

A settle that does not end fails with every cause named. The runtime answers `wait.settled`,
`wait.frames`, `audit.a11y`, `perf.start` and `perf.stop`, which are `not_answered!` today; the two
captures stay with a host that has a renderer.

### §4. A frame's internals are read in process, through types

`Tester<Headless<S>>` has what only an in-process host can give, so a cross-target scenario that
touches it does not compile:

```rust
impl<C: VirtualClock> Tester<C> {
    pub fn pump(&mut self);
    pub fn pump_for(&mut self, by: Duration);
    pub fn settle(&mut self) -> Settled<'_, C>;                       // §6
    pub fn freeze_at(&mut self, after: Duration) -> Settled<'_, C>;   // §6
}
impl<S> Tester<Headless<S>> {
    pub fn frame(&mut self) -> Frame<'_, S>;   // exclusive: nothing acts or pumps while it lives
    pub fn measure(&mut self, steps: impl FnOnce(&mut Self)) -> Cost;   // ADR-0396 §1's perf::measure
    pub fn perform(&mut self, actions: &ActionSequence) -> Result<(), TestError>;
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

`Tester<Headless<S>>::perform` plans a sequence with `input::plan` and hands it to the host's input
path. On `Unobserved` nothing resolves a locator, so a sequence there aims at points taken from a
typed read, and a target that names a locator is `TestError::NoAgent`; on `Observed` the sequence
goes to the agent through the connection. A cost scenario therefore drives input, pumps and
settles with the one vocabulary and no agent in the measured state.

### §5. Every claim has one instrument

| Claim | Instrument | Flutter 3.50.1 counterpart, or the reason to depart |
|-------|------------|------------------------------------------------------|
| Behaviour | `Tester::expect(&find, matcher)` | `expect(finder, matcher)`, `matchers.dart:46`; the matcher is wire data with one evaluator, and it holds once the condition is first true (§2) |
| Geometry | `Tester::rect` on every connection, `Frame::geometry::<R>` in process | `getRect`, `rectMoreOrLessEquals`; the protocol's `LogicalRect` gains `approx_eq` for the tolerance |
| Paint | `Frame::draw_commands -> DrawList`; `FrameRun::paint()` at the render-object depth | `paints`, `mock_canvas.dart:54`; no pattern language: Rust over an owned list |
| A render object's field | `Frame::read::<R, T>` | `renderObject<T>`, `controller.dart:987`; by `TypeId`, never a name, never on the wire |
| Pixels | `Settled::golden` (§6) | `matchesGoldenFile`, `matchers.dart:589` |
| Semantics | `Tester::read`, `Tester::semantics` | `matchesSemantics`, `matchers.dart:666` |
| Accessibility rules | `Settled::audit(&[AuditRule])`; `Tester::audit` on any connection, naming the frame it read | `meetsGuideline`, `matchers.dart:1301`; contrast from the draw commands' colours and target size from the hit region inside its clip, not from pixels (flutter/flutter#103235, #144121); each rule lives with its facts, names and target size in `flui-semantics`, contrast in `flui-rendering::audit` (the flui-driver spec's layout audit), and the runtime's `audit.a11y` and the kit both call them (ADR-0238 §10); a rule that cannot run is `cannot_verify`, never a pass |
| Time and animation | the virtual clock; `pump`, `pump_for`, `settle`, `freeze_at` | `pump`, `pumpAndSettle`, `widget_tester.dart:651`; a cause is named (#114063) and there are no zones |
| Background work | `UnsettledReason::Background`; `work().hold()` and `run()`; a seed orders ready completions | `runAsync`; gpui's seeded scheduler |
| Platform capabilities | `double::<D>()`: the double of the crate that owns the seam | `TestDefaultBinaryMessenger`; typed |
| Ownership and leaks | `DropLedger`, `Recorder<V>`, the teardown checks of §7 | `binding.dart:2638` |
| Panics and containment | `try_*` returning `TestError::FrameFailed`; one channel | `takeException`; two channels there (#151965) |
| Cost | `Tester::measure` over `flui_testing::perf::measure` and `perf::Baselines` (ADR-0396); `Tester::trace` on a device | `traceAction`, `integration_test.dart:350` |
| Robustness | `<crate>::testing::strategy`; `model::Model` over `Tester<C>`; the same model as a fuzz body; mutation score (`gamma.toml`) | none |

`Recorder<V>` wraps a view and records its lifecycle calls (builds, state creations, disposals);
it replaces `ProbeRoot`, `StateCreations` and the copied `Probe` views
(masonry's `Recorder`, `masonry_testing/src/recorder_widget.rs`).
A `strategy` module lives in the kit of the layer that owns the generated type (constraints and
sizes in `flui_rendering::testing`, geometry in `flui_foundation::testing`), because a unit test
below `flui-testing` cannot reach it (§8). A `Model` is a state machine of scenario steps with a
reference model; a failure prints the seed that reproduces it.

### §6. A golden is the product renderer's frame on one named adapter

`Settled<'_, C>` is the only receiver of `golden` and `audit`, so a frame-exact check of a frame
that is still moving cannot be written. Only a virtual clock holds a frame still, so both calls
that give one need `C: VirtualClock`: `Tester::settle`, when the list of §3 empties, and
`Tester::freeze_at(after)`, which pumps for `after` and holds the frame at that instant with its
running causes named, so a screen with a repeating animation (an activity or progress indicator)
has a golden and an audit at a stated instant and no test-only switch in the widget. On any
connection `Tester::wait_settled()` is the `wait.settled` of §3 and returns the frame number it
was answered on and its `SettleScope`, not a `Settled`.

`golden` exists on `Settled<'_, Headless<S>>`, which renders the frame's layer tree with the
engine's own `HeadlessRenderer`. `flui-testing` is in tier K, below `flui-engine`, so the kit
defines the seam (`golden::Rasterize`, a layer tree in, pixels out) and `flui-engine`'s `testing`
feature implements it; the facade's `golden` feature joins the two, and a crate below the facade
names `flui-engine` as a dev-dependency (ADR-0081's tiers stand). `AppRun` runs on a stand-in
renderer and a `Device` presents on a real one, so neither has `golden`: a device's pixels are
the driver's pixel predicates with the tolerance each call states (ADR-0238 §10), reached through
`Tester::connection_mut`.

A headless golden compares exactly, with no default tolerance, on the canonical adapter: the
software rasteriser the pull-request job installs for the readback suites (lavapipe), named in the
crate's `goldens.toml` by adapter name, driver version, its compiler's version and the vector
width it generates code for (`LP_NATIVE_VECTOR_WIDTH`, fixed for the run), and run with one
rasteriser thread. On an adapter that differs in any of the four the outcome is
`CannotVerify`, which fails a run that requires goldens and is never a pass. When the CI image
moves the rasteriser, the goldens job fails as `CannotVerify` on every pull request until one
pull request that changes only `goldens.toml` and the images re-blesses them, so drift never
lands beside a rendering change. If the experiment that precedes the golden target (30
snapshots run 100 times on the CI image and on two processor generations) shows bytes that
differ across processors on one rasteriser version, the bound it measured is written into
`goldens.toml` as that adapter's tolerance and into this section, and is never a default.

| Option weighed | Why not chosen |
|----------------|----------------|
| A tolerance on every adapter | it hides the one-pixel changes a paint regression makes, and a threshold per OS is a baseline tree per OS (egui's `crates/egui_kittest/src/snapshot.rs`; flutter/flutter#131559, #184182) |
| Baselines per platform | an author on one host cannot bless the others, and each tree drifts on its own |
| A second, CPU renderer for goldens | it certifies its own pixels instead of FLUI's (masonry's deterministic pixels come from having one renderer, `masonry_testing/src/harness.rs:564`) |

Goldens are a thin tier. A paint claim is an assertion over `DrawList` first (§5), and a crate
keeps few, small images; a text golden (an outline of a tree or a scene) is held by the same
`Goldens` holder and blessed the same way. `FLUI_BLESS=1` rewrites, and a run that failed writes
nothing. A crate's goldens are one `golden` test target that runs one test at a time.

The bounded PNG codec moves beside the one image decoder, into `flui-painting`'s `codec`
feature, and the pixel metric and the baseline store into its `pixels` feature: the kit's `golden`
feature and the driver's lanes, trace and captures call the same code, and the size of an
untrusted image is checked in one place.

### §7. A test's end is checked, and its failures have one owner

The `Tester`'s report owns the first failure, the soft failures, whether a call may have run and
the `TestError`s still alive. Every exit reads it:

- A verb that unwound, and a call answered `Unanswered`, poison the `Tester`.
- A `TestError` a `try_` verb returned is registered with the report and unregisters when it is
  dropped. A test that expects an error and goes on drops it at the end of its statement; an error
  travelling to the caller through `?` is still alive when the `Tester` drops.
- `Tester::checks(|c| ..)` adds its soft failures to the report's list and raises the list once
  when the closure returns, then clears it.
- `Tester::finish` and `Drop` on a report that is not poisoned, with no `TestError` alive and a
  thread that is not panicking, run the end checks. On a `VirtualClock` connection, in this order:
  settle (bounded as §3; a repeating cause freezes the frame at the limit and names it); audit
  that frame (`AuditRule::Unnamed` by default); unmount the root view, keeping the window and the
  runtime; run drops; release the agent's lease, which queues the release of what it holds, and
  the doubles; run turns until §3's list holds nothing the release or the unmount queued; check
  the `DropLedger`; check that §3's list is empty (a timer, a ticker or background work left
  behind); close the window and retire the runtime; raise one `TestReport`. `Connection::finish`
  carries the part a connection can perform: a `Device` releases both owners of held input (§2)
  and checks nothing it cannot read (no ledger, no exact frame), so its report names those checks
  `cannot_verify` and a device scenario audits with `Tester::audit`.
- Otherwise `Drop` releases (unmount, drops, the lease, the window) and checks nothing: the first
  failure is already on its way to the caller as a panic or as the error `?` returned, and the
  soft failures not yet raised are written to the test's output. A test that returns an error of
  its own type through `?` calls `finish` first, since no `TestError` marks that exit.

Drops run before the checks (the order flutter/flutter#144472 asks for and `binding.dart:2638`
does not have), and the harness's own objects are released before the ledger is read (#169119).
`thread::panicking()` is false after a caught panic, so a caught panic poisons through the report,
not through that flag. `finish_unchecked(reason)` and `Audit::none().because(reason)` are the
stated exceptions.

### §8. The kit's layout, the facade, and the layer kits

`flui-testing` is laid out by claim: `scenario` (the `Tester`, its connections, `find`, the
matchers, `gesture`, the diagnosis and teardown), `frame`, `audit`, `perf` and `heap` (ADR-0396's
paths), `doubles`, `ownership`, `fixture`, and behind features `golden`, `model` and `conformance`
(the suites a backend or a store author runs, out of the default build and out of the facade).
`flui::testing` re-exports all but `conformance`, with `rendering::RenderTester`. No crate is added
and no proc-macro. The host's own tests become unit tests of the kit, since `HeadlessHost` is
private.

ADR-0243's D1, D3, D4 and D7's naming rules stand. D2 stands with its list of the kit's harnesses
replaced by §1. D5 is replaced: the kit has no finder or wait of its own because the application
evaluates them (§2, §3). D6 is replaced: an exception is stated at its site, as
`#[expect(lint, reason = "…")]` where a lint holds the rule and a comment naming the reason where a
reader does, and no allowlist is kept.

During the migration each old form is `#[deprecated(note = "use Tester …")]`, `docs/gaps.md` has
the bullet for this record's unbuilt sections, and an item still on an old form carries
`#[expect(deprecated, reason = "ADR-0400: moves to Tester")]` on its `mod` line, or on the item
itself where there is no module (a bench, a unit test in `src/`). A new module on an old form
fails `clippy`, and a migrated one whose attribute stays fails as an unfulfilled expectation; a new
use inside a module that already carries the attribute is not seen by the lint, so the migration's
progress counts uses of the old names, not attributes, and the count only falls. The old forms
are listed by the task that deprecates them.

### §9. What is not built

A binding hierarchy or a global binding; a fake-async zone, `runAsync` or an `async` test body; a
wall-clock sleep in a gate; a coordinate tap that warns; a finder by type or by kind string; a
pattern language for paint; a CPU renderer for goldens; a tolerance that defaults loose;
pixel-sampled contrast; `takeException` or a global assertion hook; a matcher evaluator in the
test process; a copy of the agent's input state in the test; a `#[flui::test]` proc-macro or a
link-time test registry; a testing crate beside `flui-testing`.

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
  in CI or in the image that carries it, and a move of the rasteriser re-blesses every golden in
  a change of its own.
- Bad, because a whole-`Application` run has no `Frame`, no `measure` and no golden: the runner
  keeps its runtime in thread-local state and renders on a stand-in, so a cost scenario mounts the
  root view on `Headless`.
- Bad, because the wire has no negation and no rectangle condition, so those claims do not wait.
- Bad, because a device has no golden: its pixels are the driver's predicates with a stated
  tolerance, and a frame-exact check is written for a virtual clock.
- Bad, because a device's runner owes its half of §3 before its waits can answer `Idle`; until
  then they hold out to their limit, which is slower but never a false failure.
- Neutral, because cost scenarios run `Unobserved`, where input aims at points from typed reads:
  the agent's journal and its work after each frame are not in the measured state, and a perf file
  whose scenario moves from the agent-backed `Tester` loses that work from its counts.

### Confirmation

- The compiler: `Tester::tap(&Find<Many>)`, `Frame` on a generic `C: Connection`, `pump_for` and
  `settle` on a `Device`, and `golden` on anything but `Settled<'_, Headless<_>>` are compile-fail
  fixtures of `flui-testing`.
- clippy: `deprecated` on the old forms with the `#[expect]` marker of §8, until the last one is
  deleted.
- Tests of `flui-runtime`: a table with one row per `UnsettledReason` on which the host's settle
  and `wait(Settled)` agree, a row where both stay unsettled and name the cause and its
  `cleared_by`, and a row where both settle on the same state; a held background job never wakes
  the loop and ends a false wait `WaitEnd::Idle` naming it; a parked wait's deadline is reached
  in virtual time; a host half that is `Absent`, and an `Outside` cause, never end a wait
  `WaitEnd::Idle`; a deadline a fraction of a millisecond away is reached; a timer that re-arms
  at the same instant ends the wait unsettled after the turn bound, naming it; a redraw while a
  retry is armed wakes at the retry; one served-answer test per method removed from
  `not_answered!`; an act parked for stability injects at the bounds of the frame it passed on,
  and one past its timeout never injects.
- Tests of `flui-testing`: a read leaves the frame number unchanged; a caught panic followed by
  `Drop`, an error returned through `?` followed by `Drop`, and an expected error dropped before
  a leaked ticker, each run or skip the end checks as §7 says and lose no failure; a leaked ticker
  fails the end check while the runtime is still alive; a test that ends holding a key passes
  its end checks; `freeze_at` on an activity indicator gives a golden that reruns byte for byte; a
  chord after a `Gesture` that left a key down sends no second down; a golden off the canonical
  adapter is `CannotVerify`; a mismatch writes a diff and a failed run blesses nothing.
- A test of `flui-protocol`: one `ActionSequence` per gesture kind, planned once, gives the same
  routed events through the agent's injection, a platform lowering and a conformance row.
- One scenario of an example package runs headless in its `#[test]` and on a device in the
  nightly platform lanes through `each::<Device>`.
- `flui-driver` has no `headless` feature and no edge to `flui-testing`.

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
- Bad, because every locate goes through the agent, which costs wall time a direct pipeline read
  did not; the pilot measures it before the migration fans out.
- Bad, because the cost depth needs a second state of the headless connection (`Unobserved`) so
  that no agent sits in the measured frames.

### 5. One harness type per depth behind a shared trait of verbs with default bodies

- Good, because each depth keeps a small type, and default bodies write each verb once.
- Bad, because the default bodies are written over the trait's primitives, which are a
  connection's methods by another name, and each depth still overrides what its primitives cannot
  express (a settle, an actionability check), so the evaluators multiply where the depths differ.
- Bad, because the depth is a type an author chooses before writing the scenario, and a scenario
  over the trait names its own bound instead of the one `Tester` Flutter's author knows.

## More information

- Settle with a diagnosis: `emilk/egui@ea6b0943 crates/egui_kittest/src/lib.rs:476`
  (`ExceededMaxStepsError` with the repaint causes). Strict queries that print both matches:
  `rerun-io/kittest@11c1a3f1 src/query.rs:67`. An audit as part of settling:
  `crates/egui_kittest/src/accessibility.rs`. A no-op audit to avoid:
  `androidx/frameworks/support@f555a473 compose/ui/ui-test/src/commonMain/kotlin/androidx/compose/ui/test/Actions.kt:889`.
- A seeded order of ready work: `zed-industries/zed@c506354e crates/gpui/src/platform/test/dispatcher.rs:23`.
- An idle signal that a device cannot see: Espresso's idling resources and Detox's
  synchronization, which is why a host whose half is `Absent` holds out to the limit (§3).
- A measurement that decides a point of this record is written into this record when it is taken:
  the golden bound of §6, whether `Unobserved` stays (revisit if an attached, idle agent measures
  no difference in any counter), and whether a key locator joins the protocol.
- Revisit §6 when the engine has a CPU backend of its own: the canonical adapter becomes that
  backend and a golden checks on every host.
