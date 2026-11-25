### Added

- `flui_platform::capability::scripted` (feature `scripted`, on with `testing`): the scenario
  primitive a capability's simulated provider is built from. A `Scenario` on a virtual clock
  gives typed scripts of answers matched by request (`Script`, `Expect`, `Answer`, delays,
  `expect_held`), values the platform changes over time (`Track`, `ServiceState`) and event
  streams with a typed end (`Events`, `StreamEnd`); every answer settles at an owner turn.
  `Scenario::verify` lists what the run left undone as `ScenarioFailure`s, a script dropped with
  debts hands them to the scenario, and a scenario that drops with a failure no `verify`
  returned panics (a failure recorded after `verify` is new, even if it repeats one). Only the
  scenario's one `ScenarioDriver` holds and moves its clock. `Script::hold` hands out a
  `ScriptHold`: the script holds requests while any of its holds lives, and the last one dropped
  or `release`d sends the requests the holds kept on to the steps and the standing answer,
  oldest first and one at a time, with a request that arrives meanwhile queued behind them; a
  request a step holds waits for the test, and `ScriptHold::leave_held` lets go without sending
  anything on.
- `HeadlessHost::with_scenario`, `try_with_scenario`, `scenario` and `advance`: a host that
  drives a scenario's clock, applies its due changes whenever time moves, and waits for its
  scheduled changes in `time_to_next_frame` and `pump_until_settled`. A second host on the same
  scenario is refused (`AlreadyDriven`).
- `HostClock`: what `HeadlessHost::clock()` and `Tester::clock()` return, a clock a test reads
  and advances only through the host's scenario driver.
- `ScriptedHttp::report_to`: a request for a URL nobody scripted is also the scenario's failure
  (bounded by `MAX_REPORTS`); the headless host's own client reports to its scenario.
- `ScriptedLauncher::on`/`script`, `ScriptedFileDialogs::on`/`open_script`/`save_script` and
  `ScriptedSystem::on`/`launcher`/`dialogs`; `ScriptedLauncher::hold` and
  `ScriptedFileDialogs::hold` return the holds of their scripts.
- `flui_interaction::testing` (re-exported by `flui_testing::host`):
  `ScriptedClipboard::on`/`reads`/`service` and `ScriptedPrimarySelection::on`/`reads`/`service`.

### Changed

- `HeadlessHost::clock()` and `Tester::clock()` return `HostClock` instead of `ManualClock`:
  read it as before; advancing it applies what the host's scenario has due.

- `CompletionSource::pending` and `Resolver::resolve` no longer require the answer to be `Send`:
  a resolver is `Send` exactly when its answer is, so an owner-local answer resolves on the
  owner thread.
- `ScriptedLauncher` and `ScriptedFileDialogs` no longer answer a request nobody scripted with
  `Ok(())` or a cancel: the request's completion ends `ProviderDropped` and the scenario reports
  it as unexpected. Script the answer (`answer_next`, `answer_next_open`, `answer_next_save`).
- A held clipboard or primary-selection read still unanswered when the host's scenario ends is a
  failure of that scenario: answer it, or acknowledge it with `HeadlessHost::scenario().verify()`.
