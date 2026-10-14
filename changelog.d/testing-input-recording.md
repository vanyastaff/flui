### Added

- Input recordings (ADR-0215): a runtime emits `InputRecord` rows — each applied
  `HostInput` with its window, times and dispatch result, the arena's decisions,
  system-preference revisions, and `Gap`/`End` markers — to an `InputTap`
  registered through `UiRuntime::input_taps` or, for an application,
  `AppConfig::with_input_tap`. Tap calls are contained; a lost row is marked by a
  `Gap` and a recording is whole only with its `End` row.
- `flui_testing::recording`: JSON Lines `RecordingWriter`/`RecordingReader` over
  private mirror DTOs, text redacted unless `Privacy::KeepText`, typed
  `RecordingError`s, `record` (a worker-thread writer, inline on wasm),
  `tap_factory`, and `ReplayableRecording`, which `HeadlessHost::replay` replays
  to the decisions it recorded.
- Per-device script builders `TouchScript`, `MouseScript`, `PenScript`,
  `TrackpadScript` and `VirtualScript`, with frame steps and a pairing check;
  `HeadlessHost::run_script`, `find_center` (by semantics role and label),
  `pump_until_settled` and `set_system_preferences`;
  `A11yTree::find_by_role_and_label` and `window_bounds`.
- `ObservedMember::type_name`: the observed member's concrete type name, read
  when the decision was committed, which recorded decision rows carry.
- `cargo xtask recording-vocabulary`, a `checks` gate that keeps the generated
  table of recorded variants in step with the vocabulary.

### Changed

- `PointerScript` is built only by the per-device builders and holds steps
  (`steps`, `pointer_events`, `inputs`); `PointerScript::new`/`with`/`push`/
  `on_device`/`events` and `GestureRecorder` are gone. `HeadlessBinding::replay`
  stamps each event with its virtual time, and `replay_event` replays one.
- `HeadlessHost::pump` delivers its frame as `HostInput::Frame` at the clock's
  reading, and its wake sets the redraw flag as a runner's does.
