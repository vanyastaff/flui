# flui-runtime

The frame runtime of FLUI: the per-presentation machinery a UI realm drives
between the widget tree and a host. **Internal** — it is not an embedder API
and its surface changes with the framework, except the execution
host-injection seam (`HostExecutors` and its companions): `flui-app` and the
`flui` facade re-export it, so it carries the Stable promise (ADR-0089 §1).
Applications depend on `flui`, and hosts on `flui-app`.

## What is in it

- `epoch`: the tree revision a presentation's frames advance, and whether the
  current one has been acknowledged by a submit.
- `execution`: the host loop's background execution services (ADR-0047) —
  the compute and IO lanes, bounded admission, staged shutdown, and the
  host-injection seam `flui-app` re-exports. Only `flui-app` may depend on
  this crate, so no other workspace crate can reach the pools.
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
