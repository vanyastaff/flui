### Added

- `flui_platform::capability::scripted` (feature `scripted`, on with `testing`): the scenario
  primitive a capability's simulated provider is built from. A `Scenario` on a virtual clock
  gives typed scripts of answers matched by request (`Script`, `Expect`, `Answer`, with delays
  and holds), values the platform changes over time (`Track`, `ServiceState`) and event streams
  with a typed end (`Events`, `StreamEnd`); every answer settles at an owner turn, and
  `Scenario::verify` lists what the run left undone as `ScenarioFailure`s.
- `HeadlessHost::with_scenario` and `HeadlessHost::scenario`: a host that frames on a scenario's
  clock and applies its due changes at each pump.
- `ScriptedLauncher::on`, `ScriptedFileDialogs::on`, `ScriptedSystem::on`,
  `ScriptedClipboard::on`, `ScriptedPrimarySelection::on`, and each double's script and service
  accessors.

### Changed

- `ScriptedLauncher` and `ScriptedFileDialogs` no longer answer a request nobody scripted with
  `Ok(())` or a cancel: the request's completion ends `ProviderDropped` and the scenario reports
  it as unexpected. Script the answer (`answer_next`, `answer_next_open`, `answer_next_save`).
