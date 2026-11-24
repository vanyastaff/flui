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
- The development agent answers `audit.a11y`: `unnamed` and `target_size` over the committed
  semantics tree, `text_contrast` over the last applied frame, and every rule it cannot run
  listed in `not_checked`, so such a report is never `pass`.
- The development agent answers `perf.start` and `perf.stop` with a `PerfReport` of the frames a
  window presented between them: percentiles, the slowest frames with their phases, and dropped
  and janky frames against the display's refresh period when it is known.
- `flui_semantics::MinimumTarget` and `SemanticsOwner::audit_unnamed` / `audit_target_size`: the
  target-size rule judges the hit region inside every clip, never the drawn bounds.
- `flui_rendering::audit` and `PipelineOwner::audit_text_contrast`: WCAG text contrast read from
  a frame's scene, with a text whose background the draw commands do not determine reported as
  not checked.
- `ShapedRun::font_size`, `ShapedRun::color` and `ShapedParagraph::span_weights` in
  `flui-painting`.
- The app's development agent answers every gesture (`element.tap`, `double_tap`, `long_press`,
  `drag_to`, `swipe`, `fling`, `pinch`, `type_text`, `press_keys`, `scroll_into_view`,
  `scroll_until`) on the synthetic and semantic paths: it holds the act at frame boundaries until
  the target is attached, visible, stable over two frames, enabled and receives the hit, or the
  act's timeout passes, then injects at that frame's bounds; a refusal is `not_actionable` naming
  the failed check, and nothing is sent after an act's answer. `scroll_until` scrolls a viewport at
  a time until the element shows under the scrollable, and ends `not_found` once the content stops
  moving. A window holds at most 64 acts (`busy` past it).
- `flui_protocol::input::gestures`: the one composition of each gesture into an action sequence,
  which the agent and the driver's OS lane both play; and `Plan::tick_ends`, where each tick of a
  plan ends.

### Changed

- `HeadlessHost::time_to_next_frame` is the list's wake and `pump_until_settled` settles when the
  list names no cause; a wait an agent parked no longer keeps the host from settling.
- A wait answered on the headless host counts both halves (`SettleScope::Complete`), and one whose
  condition is false ends `idle` as soon as nothing but the test can change the application. A
  runtime whose host composes no half (the native runners) answers from its own, never idle.
- A redraw, a scheduled frame or pending work while a dropped frame's retry is armed is listed at
  the retry, and a gesture deadline not yet due is a timer, not pending work.
- The driver sends a gesture to the app's agent from its `Window`, `Element` and `ElementLocator`
  handles too, whenever the agent serves it and the path is not `os`.
- `input.inject` plays `flui_protocol::input::plan`'s timeline: its bounds and refusals are the
  plan's, each step of a move is one event stamped at its planned instant, and a move that names no
  duration lasts a step (16 ms).
- `Tester::drag` and `drag_from` hold the contact still for the protocol's swipe settle before
  the release, so a drag ends below the fling velocity on the plan's timeline.
- A settled wait on a window counts that window's causes and the application's: a redraw, a
  scheduled frame, tree work and pointer motion are listed for each window that has work for a
  frame, so another window's animation no longer keeps a still window unsettled.

### Fixed

- A gesture's reply deadline (`Addressed::holds_for`) counts the time its input takes to play after
  the act's timeout, so a long press, a slow swipe or a double tap is no longer cut short.
- A composed gesture's pointer and keyboard sources are named `gesture.mouse`, `gesture.touch`,
  `gesture.pinch.a`/`gesture.pinch.b` and `gesture.keys`, so a mouse gesture after a touch gesture on
  one input lease no longer collides on one source name.
