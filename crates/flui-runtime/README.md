# flui-runtime

The frame runtime of FLUI: the UI realm and the per-presentation machinery it
drives between the widget tree and a host. **Internal** — it is not an
embedder API and its surface changes with the framework, except the execution
host-injection seam (`HostExecutors` and its companions) and the frame-failure
report types: `flui-app` and the `flui` facade re-export them, so they carry
the Stable promise (ADR-0089 §1). Applications depend on `flui`, and hosts on
`flui-app`.

## What is in it

- `ui_realm`: `UiRealm`, the owner-affine realm — the presentations it hosts,
  their frame transaction (`render_frame` through any `FrameSink`), input
  routing, lifecycle, and the bounded command inbox other threads reach it
  through.
- `presentation`, `presentation_forest`: one presentation's owner-thread state
  (its widget tree, pipeline, gestures, focus, IME, semantics) and the
  mount-ordered set a realm hosts.
- `lifecycle_state`: the application lifecycle a presentation observes.
- `frame_failure`: what a contained frame failure reports (ADR-0048).
- `media_query_root`: the `MediaQuery` a realm installs above each root.
- `renderer_binding`: `RenderingFlutterBinding`, a presentation's rendering
  binding over its pipeline owner.
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
- `reload` (`hot-reload` feature): the development reload tier a realm
  applies.
- `testing` (`test-support` feature): a window double and a scripted sink for
  driving a realm headlessly.

## What is not

No platform backend, windowing, GPU or engine type: the runners, the platform
wiring, the raster lane and the realm dispatch layer stay in `flui-app`, which
depends on this crate. See
[ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md).
