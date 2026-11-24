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
  the driver's OS lane plays; the runtime's agent still lowers `input.inject` with its own planner.
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
  it ended (`end`: `WaitEnd::{Satisfied, Unsatisfied, Idle}`, of which no runtime answers `Idle`
  yet) and carries its `scope` (`SettleScope`); a `Placement` names its `platform` and
  `safe_area`; the `virtual` pointer device, which an agent's hello lists among the
  `capabilities.pointers` its injection delivers; `AuditRule::TextContrast`;
  `LogicalRect::approx_eq`. The method `element.scroll_until` is declared only: no app and no
  driver answers it yet.
- `Driver::run_scenario(&mut report, name, scenario)`.

### Changed

- Protocol 0.32 is breaking (`BREAKING`: `UnsettledCause.cleared_by` and `WaitOutcome.end` are
  required, `WaitOutcome.satisfied` is removed): an app speaking 0.32 refuses older clients, an
  older `flui-driver` or CLI among them, with `version`.
- Every gesture method (`element.tap`, `element.drag_to`, … `element.type_text`) is a window
  method of the agent's in the method table. No app's agent serves one yet: each answers
  `not_supported`, and the driver composes every gesture, from the agent's methods and, on the
  `os` path, its lane. A gesture an agent lists in its hello goes to that agent, unless the call
  or the session asks for the `os` path.
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

### Removed

- `WaitOutcome::satisfied`: a wait's answer says how it ended in `end` (`WaitEnd`).
- `flui_driver::report` and the driver's re-exports of `Report`, `Check`, `Outcome` and
  `Verdict`: they are `flui_protocol::report` and `flui_protocol::verdict::Verdict`.
- `flui_driver::steps::{STEP, MAX_STEPS}`: they are `flui_protocol::input::{STEP, MAX_STEPS}`.
- `Report::to_json` (serialize the report) and `Report::run_scenario` (use
  `Driver::run_scenario`).
