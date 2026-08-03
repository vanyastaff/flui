# flui-testing

**Test support for FLUI.** Today: a deterministic headless frame driver.

`flui-testing` provides `HeadlessBinding::pump_frame(dt)`: a non-singleton,
sleep-free way to advance a FLUI application by exact time steps. A virtual
`ManualClock` drives the gesture arena's clock-bound deadlines (long-press,
double-tap windows) and the frame pipeline, so time-based behavior is tested
deterministically — no real timers, no flaky sleeps.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io). It sits above `flui-widgets`
and below `flui-app` in the layer DAG: production apps use `flui-app`'s real
event loop; tests use this crate's pumped one.

```rust,ignore
let mut binding = HeadlessBinding::new(root_view);
binding.pump_frame(Duration::from_millis(300)); // advance exactly 300ms
assert!(long_press_fired.load(Ordering::SeqCst));
```

## Scope

Implemented: virtual-clock frame pumping, gesture-arena deadline polling,
restart-aware animation-controller ticking, and tree-bound build/layout/paint
frames.

This crate is the workspace's **test-support** package, not just one driver. A
widget tester, fake platform capabilities, deterministic input replay, and
golden-image helpers belong here as they land, so a test-only API never has to
be smuggled into a shipped crate behind a `testing` feature.

**Dependency rule.** Runtime and framework crates may take a *development*
edge into this crate and nothing more. A normal edge would link the test driver
into production binaries; `docs/workspace-layers.toml` records the rule and
`just inventory-check` enforces it.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-testing --open`.

## License

MIT OR Apache-2.0, per the workspace license.
