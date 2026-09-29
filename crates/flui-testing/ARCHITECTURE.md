# flui-testing Architecture

The workspace's test support, placed above the frame runtime and the widget
catalog (tier K, `order = 6`) so its driver can run the product frame
transaction ([ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) §4).
Two drivers live here while that move is in progress: `HeadlessRealm`, which
pumps a `flui_runtime::ui_realm::UiRealm`, and `HeadlessBinding`, the
substrate driver over raw owners, which the raw-owner suites still use.

## Invariants

- **A harness frame is `UiRealm::pump`.** `realm::HeadlessRealm::pump` is the
  only way the widget harness (`widgets::lay_out`, `widgets::harness::mount`)
  draws a frame, the mount included: apply commands, begin frame, the
  pipeline, end frame and the text-store commit anchor all run inside the
  realm's transaction. Nothing here re-implements a phase; the tests in
  `tests/realm_driver.rs` fail against a harness that drives the pipeline
  itself (the root `MediaQuery`, the commit gate, the owner inbox, the
  window's cursor).
- **One `ManualClock` drives the realm.** The realm takes it as its
  `ClockSource`, so the frame-time origin, the gesture arena's deadlines and
  each presentation's `FrameClock` read it, and the pump reads a clone of it
  as the frame's timestamp. Time moves only when a caller advances it:
  `HeadlessRealm::pump(dt)` before the frame, or a pointer helper's sample
  interval.
- **A contained failure is raised after the pump, and the first one wins.**
  The realm contains a segment panic or pipeline error as a dropped frame
  (ADR-0048) and reports it to the handler `HeadlessRealm` installs, with
  text retained verbatim. `pump` raises the first report once the pump has
  returned; when a later panic unwinds out of the same pump, the report is
  raised and the later payload is leaked (its destructor could panic),
  logged at error level. A lifecycle panic the tree recovered from (an
  `ErrorView`) is not raised: its frame completed. Pinned by
  `tests/headless_realm.rs` and the failure tests in `tests/realm_driver.rs`.
- **The realm root is attached once.** The widget harness attaches one
  harness root that builds whatever tree its slot holds; a root swap
  replaces the slot and rebuilds that root, so the realm's root scopes stay
  mounted and a root of the same type updates in place.
- **Input travels the realm's input path.** Pointer events go through
  `UiRealm::handle_input_addressed` inside the realm's entry; the moves it
  queues are flushed before the helper returns, so a synthetic move is
  observable at once. IME events go the same way, into the presentation's
  own text-input owner.
- **`HeadlessBinding` is the substrate driver until the second half of §4.**
  It drives raw owners (`flui-view`, scheduler, animation and interaction
  suites, the `perf` target, the facade's `tests/*.rs`) and the
  configurations a realm cannot express. Its `pump_frame`, `run_pipeline` and
  `pump_presentation`/`pump_all` go when those suites move to the pump
  (ADR-0083 `## Migration`, move 6b).
- **No shared type crosses the dev cycles.** `flui-widgets` and
  `flui-runtime` name this crate on dev edges while it depends on both, so
  their unit-test builds compile a second copy of themselves. Only
  `log_capture` crosses those edges, and it names no type of either crate.

## Mapping decisions

### `TestWidgetsFlutterBinding.pump` becomes the realm's pump

Flutter's widget tester drives a test binding whose `pump(duration)` elapses
fake time and calls `handleBeginFrame`/`handleDrawFrame` on the same binding
the app would run. Here the harness calls `HeadlessRealm::pump(dt)`, which
advances the manual clock and runs `UiRealm::pump`, the one frame
transaction every runner drives. `lay_out`'s mount is a frame, as
`pumpWidget` is: post-frame callbacks registered in `init_state` run at its
end.

Two differences follow from the realm, not from the harness. The realm's
`Vsync` registry ticks in the persistent phase with the pipeline, not among
the transient callbacks (recorded in `flui-runtime`'s `ARCHITECTURE.md`,
"`Vsync` ticks in the persistent phase, not among the transient
callbacks"); a caller's own registry passed to `lay_out_animated` is ticked
in the same phase at the same time. And the realm coalesces pointer moves
until the next frame, where Flutter's binding dispatches them at once when
resampling is off; the harness flushes the queue after each event, through
the same dispatch code the frame would run, so a test observes a move as a
Flutter test does.
