# flui-testing

**Test support for FLUI**: a deterministic headless frame driver, the canonical
mount bootstrap, virtual-clock gesture replay, and accessibility queries.

`flui-testing` provides `HeadlessBinding::pump_frame(dt)`: a non-singleton,
sleep-free way to advance a FLUI application by exact time steps. A virtual
`ManualClock` drives the gesture arena's clock-bound deadlines (long-press,
double-tap windows) and the frame pipeline, so time-based behavior is tested
deterministically — no real timers, no flaky sleeps.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io). It sits on the framework layer
beside `flui-widgets` (which takes one sanctioned optional edge into it) and
below `flui-app`: production apps use `flui-app`'s real event loop; tests use
this crate's pumped one. The reverse edge — this crate onto the widget catalog
— is forbidden as a normal dependency; the `perf` test target alone takes it as
a development edge.

```rust,ignore
let mut binding = HeadlessBinding::new();
binding.pump_frame(Duration::from_millis(300)); // advance exactly 300ms
assert!(long_press_fired.load(Ordering::SeqCst));
```

## Scope

Implemented:

- **Virtual-clock frame pumping** — `pump_frame`, gesture-arena deadline
  polling, restart-aware animation-controller ticking, the frame-driven async
  task driver, and tree-bound build/layout/paint/composite frames.
- **The canonical mount bootstrap** (`bootstrap::mount_root`) — the one way to
  get from a root `View` to mounted, rooted, laid-out owners, with the
  bootstrap frame guaranteed to be the same frame `pump_frame` runs.
- **Deterministic input replay** (`replay`) — gestures scripted as data with
  explicit virtual-time offsets, replayed by advancing the clock, so a
  long press is held and a fling is sampled the way the script says.
- **Accessibility queries** (`a11y`) — the assembled semantics tree as AccessKit
  nodes, queried by role.
- **Per-frame work counts** — `HeadlessBinding::last_frame_report()` returns a
  `FrameReport`: the build owner's report (distinct elements rebuilt, builds
  run) and the pipeline's phase counters (layout passes and roots, nodes laid
  out and painted, layers produced and reused, semantics nodes published,
  frames produced) differenced across the last pump.
- **Counted perf scenarios** (`tests/perf.rs`, its own test target) — a label
  over a lazy 10 000-row list, driven through idle, scroll, text-change and
  full-reassemble frames with budget assertions. With `FLUI_PERF_OUT=<dir>` set
  each scenario writes `<dir>/<scenario>.toml`; `cargo xtask perf` runs them and
  compares the counts with `perf/baseline.toml`.

This crate is the workspace's **test-support** package, not just one driver.
Fake platform capabilities and golden-image helpers belong here as they land,
so a test-only API never has to be smuggled into a shipped crate behind a
`testing` feature. Where layering forbids the move — `flui_widgets::testing`
mounts widgets, and this crate's library may never depend on the widget
catalog — the
harness stays put but is built on the machinery here.

**Dependency rule.** Runtime and framework crates may take a *development*
edge into this crate and nothing more. A normal edge would link the test driver
into production binaries. `cargo xtask workspace` rejects one from any lower
layer; at or above this crate's layer, where `flui-widgets` and the facade take
it only as an optional `testing` dependency, review holds the rule.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-testing --open`.

## License

MIT OR Apache-2.0, per the workspace license.
