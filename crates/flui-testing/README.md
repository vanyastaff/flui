# flui-testing

**Test support for FLUI**: a headless host that pumps the product frame
transaction, the widget test harness built on it, a deterministic substrate
driver, virtual-clock gesture replay, and accessibility queries.

`HeadlessRealm` hosts a `UiRealm` the way a runner does, over a headless
window and frame sink, and drives every frame through `UiRealm::pump` on a
virtual `ManualClock`: the realm's frame time, its gesture-arena deadlines
(long-press, double-tap windows) and its produce gate all read that one
clock, so time-based behavior is tested deterministically — no real timers,
no flaky sleeps. `flui_testing::widgets` (`lay_out`, `harness::mount`) mounts
a widget tree in one and exposes geometry, layer and element probes plus
synthetic pointer input.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io). It sits above the frame runtime
(`flui-runtime`) and the widget catalog, and below `flui-app`: production apps
use `flui-app`'s real event loop; tests pump the same realm here.

```rust,ignore
let mut laid = lay_out(GestureDetector::new().on_long_press(..).child(..), tight(100.0, 100.0));
laid.dispatch_pointer_down(50.0, 50.0);
laid.pump_for(Duration::from_millis(600)); // advance exactly 600ms of virtual time
assert!(long_press_fired.load(Ordering::SeqCst));
```

## Scope

Implemented:

- **The realm host** (`realm::HeadlessRealm`) — a `UiRealm` over a
  `HeadlessWindow` and a `HeadlessSink`, pumped on a `ManualClock`. A frame
  failure the realm contains is raised after the pump, and the first one of a
  pump stays authoritative over a later unwind.
- **The widget harness** (`widgets`) — `lay_out`/`LaidOut` for geometry,
  layers and semantics, `harness::mount`/`Harness` for element-tree probes
  and the IME session (`mount_with_ime`), `SignalProbe` for event-callback
  writes. Every frame is the realm's.
- **The substrate driver** (`HeadlessBinding::pump_frame`) — a non-singleton
  frame driver over raw owners, for the `flui-view`, scheduler, animation and
  interaction suites and for configurations a realm cannot express (no
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

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-testing --open`. The invariants live in
[`ARCHITECTURE.md`](ARCHITECTURE.md).

## License

MIT OR Apache-2.0, per the workspace license.
