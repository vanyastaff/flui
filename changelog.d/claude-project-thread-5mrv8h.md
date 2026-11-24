### Added

- `flui_protocol::client::Connection`, the one surface a test drives a window through: a typed
  `call` of a window method, `wait`, `perform`, `now` (a `ClockReading`) and `finish`, failing
  with `CallError`, whose `Unanswered { may_have_run, released }` says whether the call may have
  run and whether the agent released its input. `flui_driver::Driver` implements it over the
  window that had content when it was first used.
- `flui_protocol::report`: `Report`, `Check`, `Outcome`, `VERDICT_LINE` and `verdict_exit_code`,
  with `CheckFailure` for any error a check's attempt fails with, `Report::run`, `UnpassedCheck`
  (a check that failed or could not be made) and the report's JSON form as its `Serialize`.
- `flui_protocol::input::plan`, an action sequence planned into timed input (`TimedInput`), which
  the driver's OS lane plays; the runtime's agent still lowers `input.inject` with its own planner.
- `GestureOptions::named_timeout_ms`, an act's named timeout bounded at a day.
- `flui_driver::Awaited` on `Error::Timeout`: whether the app's agent or a tool on the host did
  not answer. Only the agent's becomes `CallError::Unanswered`; a hung host tool is
  `CallError::Failed` with its message.
- `ScriptedLane::hanging_sends`, a scripted lane whose every send times out.
- Protocol 0.32: `Locator::root`; `Expectation::Visible` and `Expectation::Description`; an
  `UnsettledCause` names its `window`, `owner`, `site` and what clears it (`ClearedBy`, a required
  field), with the reasons `ticker`, `background`, `owed_turn`, `window_observation`,
  `frame_request`, `scenario_step`, `surface_frame` and `platform_reply`; a `WaitOutcome` says how
  it ended (`end`: `WaitEnd::{Satisfied, Unsatisfied, Idle}`, of which no runtime answers `Idle`
  yet) and carries its `scope` (`SettleScope`); a `Placement` names its `platform` and
  `safe_area`; the `virtual` pointer device; `AuditRule::TextContrast`; `LogicalRect::approx_eq`.
  The method `element.scroll_until` is declared only: no app and no driver answers it yet.
- `Driver::run_scenario(&mut report, name, scenario)`.

### Changed

- Every gesture method (`element.tap`, `element.drag_to`, … `element.type_text`) is a window
  method of the agent's in the method table. No app's agent serves one yet: each answers
  `not_supported`, and the driver composes every gesture, from the agent's methods and, on the
  `os` path, its lane. A gesture an agent lists in its hello goes to that agent, unless the call
  or the session asks for the `os` path.
- A composed gesture's act timeout is bounded at a day, as the window's hold is.
- The driver's OS lane plays `input::plan` and refuses, before any event, what the plan refuses:
  more than 32 sources, more than 4096 ticks, two sources of one name, a point more than 1e9
  logical pixels from the origin or not finite, a pressure outside `0..=1`, a non-finite delta or
  trackpad value, and a sequence that plays longer than ten minutes, a step for each move that
  names no duration included. It also refuses a scroll by lines (`os_line_scroll`), which no lane
  can size, a virtual pointer, a pointer's cancel and a trackpad gesture.
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
