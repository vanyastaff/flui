# flui-binding

**Deterministic headless frame driver for FLUI tests.**

`flui-binding` provides `HeadlessBinding::pump_frame(dt)`: a non-singleton,
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

Implemented: virtual-clock frame pumping and gesture-arena deadline polling.
Deferred (tracked in `docs/ROADMAP.md`): animation-controller ticks (Phase 3)
and tree-rebuild integration (Phase 1b).

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-binding --open`.

## License

MIT OR Apache-2.0, per the workspace license.
