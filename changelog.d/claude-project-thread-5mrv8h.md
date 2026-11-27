### Added

- `flui_protocol::client::Connection`, the one surface a test drives a window through: a typed
  `call` of a window method, `wait`, `perform`, `now` (a `ClockReading`) and `finish`, failing
  with `CallError`, whose `Unanswered { may_have_run, released }` says whether the call may have
  run and whether the agent released its input. A connection drives one window: a call whose
  parameters name no window is sent to it, one naming another is refused `invalid_argument`. A
  read runs no frame; an act or a wait may run frames and move the clock.
  `flui_driver::Driver` implements it over the window that had content when it was first used;
  once that window closes every operation answers `gone`, and the end counts a closed window as
  holding no input.
- `flui_protocol::report`: `Report`, `Check`, `Outcome`, `VERDICT_LINE` and `verdict_exit_code`,
  with `CheckFailure` for any error a check's attempt fails with, `Report::run`, `UnpassedCheck`
  (a check that failed or could not be made) and the report's JSON form as its `Serialize`.
- `flui_protocol::input::plan`, an action sequence planned into timed input (`TimedInput`), which
  the driver's OS lane and the app's agent play.
- `ActionSequence::plays_within`: a sequence's span and a frame (`input::STEP`) for each tick,
  what an injection's reply is bounded by past the agent's own timeout.
- `GestureOptions::named_timeout_ms`, an act's named timeout bounded at a day.
- `flui_driver::Awaited` on `Error::Timeout`: whether the app's agent or a tool on the host did
  not answer. Only the agent's becomes `CallError::Unanswered`; a hung host tool is
  `CallError::Failed` with its message.
- `Launch::frame_period`: how far each frame moves a headless app's virtual clock, 16 ms unless
  named, so a test can stand for a window that presents more slowly.
- `ScriptedLane::hanging_sends`, a scripted lane whose every send times out.
- Protocol 0.32: `Locator::root`; `Expectation::Visible` and `Expectation::Description`; an
  `UnsettledCause` names its `window`, `owner`, `site` and what clears it (`ClearedBy`, a required
  field), with the reasons `ticker`, `background`, `owed_turn`, `window_observation`,
  `frame_request`, `scenario_step`, `surface_frame` and `platform_reply`; a `WaitOutcome` says how
  it ended (`end`: `WaitEnd::{Satisfied, Unsatisfied, Idle}`) and carries its `scope`
  (`SettleScope`); a `Placement` names its `platform` and
  `safe_area`; the `virtual` pointer device, which an agent's hello lists among the
  `capabilities.pointers` its injection delivers; `AuditRule::TextContrast`;
  `LogicalRect::approx_eq`; the method `element.scroll_until`.
- `Driver::run_scenario(&mut report, name, scenario)`.
- `flui_testing::scenario` (`flui::testing::scenario`): `Tester<C>` over the
  in-process `Headless` connection (ADR-0400). Its finders are `Find<One>` and `Find<Many>`, so a
  gesture aimed at many elements does not compile. Its matchers are data that `Tester::expect`
  waits on. A read runs no frame, and an act ends with an owner turn that drains what its frames
  queued. Each verb has a `try_` twin returning a `TestError` with its `Diagnosis`. The end of a
  test (`finish`, `try_finish`, or `Drop`, which only reports) settles, audits unnamed elements,
  unmounts the root, releases held input and checks the `DropLedger` and that no timer, ticker or
  background work was left behind. It skips those checks, and says why, while a failure is on its
  way to the caller.
- Constructors for the gesture parameters of `flui-protocol` (`LongPressParams::new`,
  `DragParams::new`, `SwipeParams::new`, `ScrollIntoViewParams::new`, `TypeTextParams::new`,
  `PressKeysParams::new`).
- **Typed frame reads in process** (`flui-testing`, `flui::testing::frame`; ADR-0400 §4):
  `Tester::frame` on a `Tester<Headless<_>>` lends a `Frame`, whose reads copy owned values out of
  the frame the window presented last: `only::<R>` and `render_all::<R>` for the render objects of
  a type (`RenderGeometry`: size, sliver geometry, offset and the transform into the window),
  `geometry::<R>`, `read::<R, T>` and `draw_commands` at the element a `Find` names, `draw_list`,
  `layers` (`LayerOutline`) and `report`, the `FrameReport` of the tester's last move of its
  clock (`pump`, `pump_for`, `freeze_at`). A scenario over a generic `Connection` that calls
  `frame` does not compile. A read of one render object fails with `TestError::RenderObjectCount`
  when it finds another number, and a read at an element no render object forms with
  `TestError::NoRenderObject`.
- **Gestures** (`flui-testing`, `flui::testing::scenario::gesture`; ADR-0400 §2): `Gesture`,
  `Pointer` and `Keys` build the protocol's `ActionSequence` for several pointers, keys and text,
  with a press held for a stated time (`Pointer::hold`); `try_build` refuses a press the gesture
  leaves down or a release it did not press (`GestureError`). `Tester::perform` and
  `try_perform` send it through the window's input lease.
- **A tester with no agent** (`flui-testing`; ADR-0400 §1): `Tester::mount_unobserved` mounts a
  view on a `Headless<Unobserved>` target, which attaches no development agent; it pumps on
  the virtual clock, reads its frame and ends by settling, unmounting and checking what was left
  behind. `Tester::measure` measures the steps it runs with `flui_testing::perf::measure`, and
  counts each frame as the widget harness's `pump_report` does. It returns a `Cost`, which the
  facade exports as `flui::testing::Cost`.

### Changed

- Protocol 0.32 is breaking (`BREAKING`: `UnsettledCause.cleared_by` and `WaitOutcome.end` are
  required, `WaitOutcome.satisfied` is removed): an app speaking 0.32 refuses older clients, an
  older `flui-driver` or CLI among them, with `version`.
- Every gesture method (`element.tap`, `element.drag_to`, … `element.type_text`) is a window
  method of the agent's in the method table. A gesture an agent lists in its hello goes to that
  agent, unless the call or the session asks for the `os` path; the driver composes the others,
  from the agent's methods and, on the `os` path, its lane.
- A composed gesture's act timeout is bounded at a day, as the window's hold is.
- An injection's reply is bounded by `ActionSequence::plays_within`, a frame a tick on top of
  its span, so a long run of ticks that name no duration is no longer answered `timeout` while it
  plays.
- `Driver::capabilities().pointers` lists what the agent's hello says its injection delivers;
  for an agent older than 0.32, which lists none, a mouse, a touch contact and a pen.
- `Addressed` has a required `address(window)`, which names a window where the parameters name
  none.
- The driver's OS lane plays `input::plan` and refuses, before any event, what the plan refuses:
  more than 32 sources, more than 4096 ticks, two sources of one name, a point more than 1e9
  logical pixels from the origin or not finite, a pressure outside `0..=1`, a delta or angle not
  finite, a pinch's scale not finite or not above zero, and a sequence that plays longer than ten
  minutes, a step for each move that names no duration included. It now also refuses a scroll by
  lines (`os_line_scroll`), which no lane can size, and a virtual pointer. These refusals, and
  the refusal of a pointer's cancel and a trackpad gesture, come before the desktop is locked or
  the window located.
- `UnsettledCause::new(reason)` is `UnsettledCause::new(reason, cleared_by)`, and
  `UnsettledCause` is no longer `Copy`.
- `Error::Timeout` carries `awaited`.
- A trace replays only against the protocol it was recorded at: `trace::replay` refuses another
  with `Error::Version` before any command.
- `window.placement` answers the window's platform convention and its safe area.
- An act of the development agent leaves a key its input lease held as the act started as it
  was: a chord naming it sends no second press and no release. `flui_testing::tester::Tester`'s
  `press_keys` is now that act, and the tester keeps no record of held keys of its own.
- The book's tutorial chapters, the testing cookbook page and the `Tester` row of the Flutter
  mapping teach `flui::testing::scenario::Tester`: `find` builds a `Find` that names one element,
  and `t.expect(&find, exists())` takes a matcher.

### Deprecated

- `flui_testing::tester::{Tester, Expectation, TestError}` and the `Locator`-returning functions
  of `flui_testing::find`, also re-exported at `flui::testing`'s root: use
  `flui_testing::scenario`.

### Removed

- `WaitOutcome::satisfied`: a wait's answer says how it ended in `end` (`WaitEnd`).
- `flui_driver::report` and the driver's re-exports of `Report`, `Check`, `Outcome` and
  `Verdict`: they are `flui_protocol::report` and `flui_protocol::verdict::Verdict`.
- `flui_driver::steps::{STEP, MAX_STEPS}`: they are `flui_protocol::input::{STEP, MAX_STEPS}`.
- `Report::to_json` (serialize the report) and `Report::run_scenario` (use
  `Driver::run_scenario`).
