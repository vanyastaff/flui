# flui-testing

**Test support for FLUI**: a headless host that pumps the product frame
transaction, the widget test harness built on it, a deterministic substrate
driver, virtual-clock gesture replay, and accessibility queries.

`HeadlessHost` hosts a `UiRuntime` the way a runner does, over a headless
window and frame sink, and drives every frame through `UiRuntime::pump` on a
virtual `ManualClock`: the UI runtime's frame time, its gesture-arena deadlines
(long-press, double-tap windows) and its produce gate all read that one
clock, so time-based behavior is tested deterministically — no real timers,
no flaky sleeps. `flui_testing::widgets` (`lay_out`, `harness::mount`) mounts
a widget tree in one and exposes geometry, layer and element probes plus
synthetic pointer input.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io). It sits above the frame runtime
(`flui-runtime`) and the widget catalog, and below `flui-app`: production apps
use `flui-app`'s real event loop; tests pump the same UI runtime here.

```rust,ignore
let mut laid = lay_out(GestureDetector::new().on_long_press(..).child(..), tight(100.0, 100.0));
laid.dispatch_pointer_down(50.0, 50.0);
laid.pump_for(Duration::from_millis(600)); // advance exactly 600ms of virtual time
assert!(long_press_fired.load(Ordering::SeqCst));
```

## Scope

Implemented:

- **The headless host** (`host::HeadlessHost`) — a `UiRuntime` over a
  `HeadlessWindow` and a `HeadlessSink`, pumped on a `ManualClock`. A frame
  failure the UI runtime contains is raised after the pump, and the first one of a
  pump stays authoritative over a later unwind.
- **The widget harness** (`widgets`) — `lay_out`/`LaidOut` for geometry,
  layers and semantics, `harness::mount`/`Harness` for element-tree probes
  and the IME session (`mount_with_ime`), `SignalProbe` for event-callback
  writes. Every frame is the UI runtime's.
- **The substrate driver** (`HeadlessBinding::pump_frame`) — a non-singleton
  frame driver over raw owners, for the `flui-view`, scheduler, animation and
  interaction suites and for configurations a UI runtime cannot express (no
  post-frame lanes, no root `MediaQuery` or `VsyncScope`). It moves onto the
  pump in a later change (ADR-0083 §4).
- **The canonical mount bootstrap** (`bootstrap::mount_root`) — the one way to
  get from a root `View` to mounted, rooted, laid-out owners on the substrate
  driver.
- **Deterministic input replay** (`replay`) — gestures scripted as data with
  explicit virtual-time offsets, replayed by advancing the clock, so a
  long press is held and a fling is sampled the way the script says.
- **Accessibility queries** (`a11y`) — the assembled semantics tree as AccessKit
  nodes, queried by role.
- **The text-store conformance kit** (`text_store_kit`, ADR-0090).
- **Per-frame work counts** — `HeadlessBinding::last_frame_report()` returns a
  `FrameReport`: the build owner's report and the pipeline's phase counters
  differenced across the last pump. The counted perf scenarios built on it
  live in `flui-widgets`' `perf` test target (see `cargo xtask perf`).

This crate is the workspace's **test-support** package, not just one driver.
Fake platform capabilities and golden-image helpers belong here as they land,
so a test-only API never has to be smuggled into a shipped crate behind a
`testing` feature.

**Dependency rule.** Runtime and framework crates may take a *development*
edge into this crate and nothing more; a normal edge would link the test
driver into production binaries. `cargo xtask workspace` rejects one from any
crate below it, and `flui-widgets` forbids reaching it (`reach-forbid`). The
facade takes it only as an optional `testing` dependency.

## Checking an interaction

Establish the initial state, identify one subject, dispatch the event, then
assert the changed state after pumping the required frames. `A11yTree::find`
and `find_by_label` refuse ambiguous subjects; an indexed `find_all` result
does not establish uniqueness. Check the subject's value, toggle state or
geometry, rather than merely confirming that a textbox or button exists.

A delivered semantics action is not a completed effect. A `GestureDetector`
can defer its tap callback to a later frame, and a callback can schedule a
rebuild for the following frame. Advance time deliberately for animation or
gesture deadlines; a zero-duration pump does not advance the virtual clock,
and one pump is not a general guarantee that work has settled. Keep finite
frame/time bounds when waiting for an observable condition.

The harness checks product build, layout, paint recording, semantics assembly
and synthetic input on bundled fonts. Its sink records scenes without a GPU
or native compositor, so recorded paint does not prove visible pixels or
presentation. GPU readbacks need the engine tests; system-font fallback,
native event translation, IME integration and OS accessibility need native
checks. `A11yTree` exposes the raw root-reachable AccessKit payload, including
hidden and transparent nodes. Platform adapters and the development agent
apply additional filtering before exposing their trees.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-testing --open`. The invariants live in
[`ARCHITECTURE.md`](ARCHITECTURE.md).

## License

MIT OR Apache-2.0, per the workspace license.
