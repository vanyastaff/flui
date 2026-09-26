# flui-runtime

The frame runtime of FLUI: the per-presentation machinery a UI realm drives
between the widget tree and a host. **Internal** — it is not an embedder API
and its surface changes with the framework. Applications depend on `flui`, and
hosts on `flui-app`.

## What is in it

- `epoch`: the tree revision a presentation's frames advance, and whether the
  current one has been acknowledged by a submit.
- `held_input`: the bounded pointer input a presentation retains while it has
  no committed tree, and the replay that delivers it once one exists.
- `performance_stats`: the rolling frame-time window a presentation's
  performance overlay draws.
- `semantics_host`: per-presentation semantics enablement and platform
  accessibility delivery.
- `sink`: the `FrameSink` a host implements and a frame is submitted through,
  and the `SubmitVerdict` the realm classifies.

## What is not

No platform backend, windowing, GPU or engine type: the runners, the platform
wiring and the raster lane stay in `flui-app`, which depends on this crate.
See [ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md).
