### Added

- `flui_runtime::settledness::{Unsettled, HostHalf, TURNS_AT_ONE_INSTANT}`: one list of what keeps
  an application from being idle at an instant, each cause added through the method of what
  discharges it (`next_frame`, `deadline` with its exact instant, `test`, `outside`), and
  `Unsettled::wake`, the one wake rule a host's loop follows (ADR-0400 §3).
- `UiRuntime::unsettled` (the runtime's half) and `UiRuntime::answer_settled`, which answers the
  agent's waits that read settledness, every wait's timeout and the idle end from the host's
  list; `RuntimeHostServices::composes_host_half` says a host composes its half.
- The development agent answers `wait.settled` and `wait.frames`.
- `HeadlessHost::unsettled`, the host's whole list, and `HeadlessHost::run_until_answered`, which
  runs turns for a parked request on the virtual clock, moving it to the list's wake.

### Changed

- `HeadlessHost::time_to_next_frame` is the list's wake and `pump_until_settled` settles when the
  list names no cause; a wait an agent parked no longer keeps the host from settling.
- A wait answered on the headless host counts both halves (`SettleScope::Complete`), and one whose
  condition is false ends `idle` as soon as nothing but the test can change the application. A
  runtime whose host composes no half (the native runners) answers from its own, never idle.
- A redraw, a scheduled frame or pending work while a dropped frame's retry is armed is listed at
  the retry, and a gesture deadline not yet due is a timer, not pending work.
