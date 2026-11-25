### Added

- `flui_runtime::settledness::{Unsettled, HostHalf, TURNS_AT_ONE_INSTANT}`: one list of what keeps
  an application from being idle at an instant, each cause added through the method of what
  discharges it (`next_frame`, `deadline` with its exact instant, `test`, `outside`), with
  `Unsettled::wake_at` and `Unsettled::wake_at_once` for what only wakes the loop, and
  `Unsettled::wake`, the wake rule `HeadlessHost` follows; the native runners' loops still wake
  from `UiRuntime::next_wake_owing_floors` until they compose a half (`docs/gaps.md`).
- `UiRuntime::unsettled` (the runtime's half) and `UiRuntime::answer_settled`, which answers the
  agent's waits that read settledness, every wait's timeout and the idle end from the host's
  list; `UiRuntime::awaits_settled_answer`, whether such a wait is parked, so a host gathers its
  list only then; `RuntimeHostServices::composes_host_half` says a host composes its half, and
  `RuntimeHostServices::with_floor_owed` gives the host's rule for which in-flight floors it owes,
  read by `UiRuntime::next_wake` and `UiRuntime::unsettled` alike; `UiRuntime::list_for_next_turn`
  is the one rule for what a dropped frame's retry holds, which the headless host's half follows
  too.
- The development agent answers `wait.settled` and `wait.frames`.
- `HeadlessHost::unsettled`, the host's whole list, and `HeadlessHost::run_until_answered`, which
  runs turns for a parked request on the virtual clock, moving it to the list's wake.
- The development agent answers `audit.a11y`: `unnamed` and `target_size` over the committed
  semantics tree, `text_contrast` over the last applied frame, and every rule it cannot run
  listed in `not_checked`, so such a report is never `pass`. A target-size finding names the
  pointer it was judged for, the window's primary pointer; a disabled control's text is listed in
  `not_checked`, since an inactive control's text has no contrast requirement.
- The development agent answers `perf.start` and `perf.stop` with a `PerfReport` of the frames a
  window presented between them: percentiles of each frame's work from build to the end of paint
  (the submit and the vsync wait a blocking present holds are not part of it), the slowest frames
  with their phases, and dropped frames (work past one refresh period) and janky ones (dropped
  frames past `jank_multiplier` periods, counted in both) against the period of the display each
  frame presented on, when it is known. A span is its caller's: the calls are asked through an
  input lease, a window keeps one span per caller (at most eight, `busy` past that), and a span
  ends with its connection.
- `flui_semantics::MinimumTarget` and `SemanticsOwner::audit_unnamed` / `audit_target_size`: the
  target-size rule judges the hit region inside the window and every clip that does not scroll,
  so a row partly scrolled out of a list is judged by its size once revealed; it does not judge an
  inline link (a `link` beside text in its parent) nor a target inside an interactive ancestor
  whose hit region meets the minimum.
- `flui_rendering::audit` and `PipelineOwner::audit_text_contrast`: WCAG text contrast read from
  a frame's scene and compared unrounded, with a text whose background the draw commands do not
  determine reported as not checked and a fully transparent glyph run not judged.
- `ShapedRun::font_size`, `ShapedRun::color` and `ShapedParagraph::span_weights` in
  `flui-painting`.
- The app's development agent answers every gesture (`element.tap`, `double_tap`, `long_press`,
  `drag_to`, `swipe`, `fling`, `pinch`, `type_text`, `press_keys`, `scroll_into_view`,
  `scroll_until`) on the synthetic and semantic paths: it holds the act at frame boundaries until
  the target is attached, visible, stable over two frames, enabled and receives the hit, or the
  act's timeout passes, then injects at that frame's bounds; a refusal is `not_actionable` naming
  the failed check, and nothing is sent after an act's answer. Every act is answered by its
  deadline, the input it queued included: input no frame started by then is withdrawn and the act
  answers `timeout`; input under way has its play time past the deadline. `scroll_until` scrolls a
  viewport at a time until the element shows under the scrollable, and ends `not_found` once the
  content stops moving. A window holds at most 64 acts (`busy` past it).
- `flui_protocol::input::gestures`: the one composition of each gesture into an action sequence,
  which the agent and the driver's OS lane both play; and `Plan::tick_ends`, where each tick of a
  plan ends.
- `flui_protocol::input::gestures::Defaults`: what a gesture takes where its call names no
  duration, velocity or distance and a window's settings decide it, and the pointer a tap, double
  tap and drag play with. The app's agent fills it from the window's own `GestureSettings`, so a
  default long press outlasts that window's long-press timeout, a default fling releases above its
  fling velocity, and a tap, double tap and drag play with the window's primary pointer (touch on
  Android, iOS and Fuchsia, a mouse elsewhere); a composer with no window takes
  `Defaults::WINDOWLESS`. A window whose long-press timeout asks for a press past
  `Defaults::LONGEST_LONG_PRESS_MS` refuses a long press naming no duration, naming the duration to
  give.
- `flui_protocol::input::gestures::chord_keys`, the one reader of a chord such as `Control+a`.
- `flui_protocol::locator::resolve_strict`, `STRICT_CANDIDATES` and `Ambiguity`: one rule for a
  strict locator's pick, which the agent and the driver both follow, counting every match.
- `GestureSettings::fling_velocity_range_for`: the release speeds a pointer kind flings between,
  the host's for a touch where it reports them.
- `flui_semantics::element_id_of`, the wire handle of the element a render object contributes.

### Changed

- `HeadlessHost::time_to_next_frame` is the list's wake and `pump_until_settled` settles when the
  list names no cause; a wait an agent parked no longer keeps the host from settling.
- A wait answered on the headless host counts both halves (`SettleScope::Complete`), and one whose
  condition is false ends `idle` as soon as nothing but the test can change the application. A
  runtime whose host composes no half (the native runners) answers from its own, never idle.
- What the development agent asks for itself (a held act's frames, a frame-counting wait's frames,
  an owed reading) is a wake, never a cause: a false wait ends `idle` and the host settles while an
  act is held. A wait that counts frames never ends `idle`. Input an act queued is the
  application's work until it has played.
- A settled wait on a window counts that window's causes and the application's: a redraw, a
  scheduled frame, tree work and pointer motion are listed for each window whose work the next
  frame serves. A hidden window, one whose sink is full and one whose device was given up are named
  for none, so another window's animation no longer keeps a still or hidden window unsettled.
- A cause of a window whose dropped frame's retry is armed is listed at that retry; another
  window's causes, and the application's, are the next turn's. A queued command or owner job is
  listed as `PendingWork` of the application, and a gesture deadline not yet due is a timer.
- The runtime's half lists a window's in-flight floor only where its host owes it.
- The driver chooses a gesture's path once, from the call's `via`, the session's policy and the
  lane, and a gesture the app's agent serves goes to it carrying that path as its `via`, required
  when the policy requires it; a call that prefers `os` with no lane falls back to synthetic and is
  the agent's. This holds for the driver's `Window`, `Element` and `ElementLocator` handles and for
  `element.scroll_until`.
- `input.inject` plays `flui_protocol::input::plan`'s timeline: its bounds and refusals are the
  plan's, each step of a move is one event stamped at its planned instant, and a move that names no
  duration lasts a step (16 ms).
- `gestures::drag` holds still before it lifts, so a drag is dropped where it ends, and refuses a
  drag inside the pointer's slop (`GestureError::DragTooShort`); `Tester::drag` and `drag_from` hold
  still the same `SWIPE_SETTLE_MS`. `gestures::tap`, `double_tap` and `drag` take `&Defaults`.
- A default fling over a stroke too short to measure a release speed is refused naming the
  distance or velocity to give (`GestureError::FlingTooShort`); a pinch inside the window's scale
  slop is refused.
- `SemanticsOwner::actionability_wire` returns an `ActionabilityReading`: the facts the wire
  carries and the part of the element the window shows. The agent aims a gesture's extent at that
  part: a swipe naming no distance goes most of the way from its point to the edge of the part
  shown, and a pinch's radius is that part's.
- `Tester::press_keys` reads its chord with `gestures::chord_keys` and refuses one that does not
  read (`Control+`, `a++b`, a spaced `Control + a`) instead of pressing part of it.

### Fixed

- A gesture's reply deadline (`Addressed::holds_for`) counts the time its input takes to play after
  the act's timeout, so a long press, a slow swipe or a double tap is no longer cut short.
- A composed gesture's pointer and keyboard sources are named `gesture.mouse`, `gesture.touch`,
  `gesture.pinch.a`/`gesture.pinch.b` and `gesture.keys`, so a mouse gesture after a touch gesture on
  one input lease no longer collides on one source name.
- `MinimumTarget::admits` meets the minimum to within 0.005 logical px, so a target laid out at
  exactly the minimum is no longer a finding at device ratios such as 1.1, 1.3 or 1.4.
- A strict locator matched by more than ten elements reports its true count, from a gesture the
  agent plays and from one the driver composes.
- A wait that keeps frames coming parks nothing when the wake it asks for panics, and a wake that
  panics for one window no longer cuts the wait answer pass short.
- A held act's last attempt on a turn without a frame names the check that failed, not `stable`;
  two readers of one frame get the same stability answer.
