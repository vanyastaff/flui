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
  `ErrorView`) is not raised: its frame completed. A report the realm makes
  between pumps is raised by the next pump before it frames, not erased.
  After any raise, including a pump that unwound past its commit anchor, the
  next pump frames and runs the grants the unwound one queued. Pinned by
  `tests/headless_realm.rs`, `realm::tests` and the failure tests in
  `tests/realm_driver.rs`.
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
- **Only `log_capture` crosses the dev cycles.** `flui-widgets` and
  `flui-runtime` name this crate on dev edges while it depends on both, so
  their unit-test builds compile a second copy of themselves. Their unit
  tests use only `log_capture`, which names no type of either crate. The
  compiler does not refuse more: a `flui-widgets` unit test could hand
  `widgets::lay_out` a view (the `View` trait lives in `flui-view`, of which
  there is one copy), and its lookups of `MediaQuery`, `FocusRoot` or
  `VsyncScope` would then silently miss the scopes the realm installed from
  the other copy. Review keeps harness tests in `tests/`.
- **The realm stays behind its host.** `HeadlessRealm::realm` and
  `HeadlessRealm::enter` are crate-private, and `LaidOut` and `Harness` hand
  out narrow accessors (the window's cursor, the accessibility action
  listener, the post-frame handle, the scheduler) rather than the realm:
  `flui-runtime` is not an embedder API, and `flui::testing` re-exports
  `widgets`, so a public path to `UiRealm` here would open the realm's frame
  entry points (ADR-0083 §2) to every application with the `testing`
  feature.

## Mapping decisions

### The harness pumps the realm

The harness calls `HeadlessRealm::pump(dt)`, which
advances the manual clock and runs `UiRealm::pump`, the one frame
transaction every runner drives. `lay_out`'s mount is a frame:
post-frame callbacks registered in `init_state` run at its
end.

Two behaviors follow from the realm, not from the harness. The realm's
`Vsync` registry ticks in the persistent phase with the pipeline, not among
the transient callbacks (recorded in `flui-runtime`'s `ARCHITECTURE.md`,
"`Vsync` ticks in the persistent phase, not among the transient
callbacks"); a caller's own registry passed to `lay_out_animated` is ticked
in the same phase at the same time. And the realm coalesces pointer moves
until the next frame; the harness flushes the queue after each event, through
the same dispatch code the frame would run, so a test observes a move
immediately.

### Logical render roots follow reconciliation

`LaidOut::root`, `current_root` and render-type traversal resolve the caller's
live render subtree after every frame. A composition root remains mounted while
its child changes render type or a contained build failure installs an error
view; a cached render ID would then point at the detached child. The
`logical_render_root_tracks_replacement_and_build_recovery` row in
`headless_frame_driver_matrix` checks healthy replacement, the rendered error
slot and recovery through the root's own rebuild handle.

### The conformance kit retains caught opaque panic payloads

`Case::run` borrows its fixture and converts a caught panic into a named
`CaseFailure`. After extracting string diagnostics, it retains the opaque
payload: arbitrary aggregate destructors may panic twice during one Drop. This
exceptional retention does not retain the fixture or alter normal fixture
teardown. The isolated `kit_retains_opaque_failure_payloads_and_continues` row
in `text_store_kit_matrix` injects two independent aggregate payloads through
public fixture reset, checks both named failures, then runs the kit successfully
again. Neither aggregate destructor executes.

The kit's deliberately panicking-grant case also confirms that the supplied
grant actually ran. A store panic before invocation is reported through the
outer case boundary; its payload is retained before any next grant. The
`kit_reports_pre_grant_failure_without_destroying_its_payload` isolated row
exercises that distinction and then issues a successful grant on the same
consumer store. Store ownership remains with the fixture in these regressions;
normal arbitrary store destructor behavior is governed by Rust's unwind rules.

### A reused signal probe follows its current mount

Each build refreshes the probe's observed signal and reactive graph together.
A sequential remount creates a new signal, so retaining the first pair would
read a released slot while the new mount remained live. Old graph retirement
occurs outside the observation cell's borrow.
`a_signal_probe_reads_and_writes_its_current_mount_after_remount` in
`headless_frame_driver_matrix` writes through actual pointer callbacks before
and after remount, and reads the new initial value between them.
