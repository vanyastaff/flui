### Added

- `flui_protocol::client::Connection`, the one surface a test drives a window through: a typed
  `call` of a window method, `wait`, `perform`, `now` and `finish`, failing with `CallError`,
  whose `Unanswered { may_have_run, released }` says whether the call may have run and whether
  the agent released its input. `flui_driver::Driver` implements it.
- `flui_protocol::report`: `Report`, `Check`, `Outcome`, `VERDICT_LINE` and `verdict_exit_code`,
  with `CheckFailure` for any error a check's attempt fails with, `Report::run`, `Failure` (a
  check that did not pass) and the report's JSON form as its `Serialize`.
- `flui_protocol::input::plan`, the one lowering of an action sequence into timed input, which
  the driver's OS lane now plays.
- Protocol 0.32: `Locator::root`; `Expectation::Visible` and `Expectation::Description`; an
  `UnsettledCause` names its `window`, `owner`, `site` and what clears it (`ClearedBy`, a required
  field), with the reasons `ticker`, `background`, `owed_turn`, `window_observation`,
  `frame_request`, `scenario_step`, `surface_frame` and `platform_reply`; a `WaitOutcome` carries
  its `scope` (`SettleScope`); a `Placement` names its `platform` and `safe_area`; the `virtual`
  pointer device; `AuditRule::TextContrast`; the method `element.scroll_until`;
  `LogicalRect::approx_eq`.
- `Driver::run_scenario(&mut report, name, scenario)`.

### Changed

- Every gesture method (`element.tap`, `element.drag_to`, … `element.type_text`) is a window
  method the agent serves, held for its act timeout; the driver composes it while the agent does
  not answer it, and on the `os` path.
- The driver's OS lane refuses an action sequence longer than ten minutes, as the agent does.
- `window.placement` answers the window's platform convention and its safe area.

### Removed

- `flui_driver::report` and the driver's re-exports of `Report`, `Check`, `Outcome` and
  `Verdict`: they are `flui_protocol::report` and `flui_protocol::verdict::Verdict`.
- `Report::to_json` (serialize the report) and `Report::run_scenario` (use
  `Driver::run_scenario`).
