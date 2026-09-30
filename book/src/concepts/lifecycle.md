# Lifecycle

Every `Element` moves through a fixed set of lifecycle states, defined as
`enum Lifecycle` in `crates/flui-view/src/element/lifecycle.rs`:

```text
Initial → Active ⇄ Inactive → Defunct
```

- **`Initial`** — created but not yet mounted into the tree.
- **`Active`** — mounted, participating in builds, can be marked dirty, has valid parent/child
  relationships.
- **`Inactive`** — temporarily removed from the tree; state is preserved and the `Element` may be
  reactivated within the same frame (its `RenderObject`, if any, is detached but not disposed).
- **`Defunct`** — permanently removed; the `Element` will be dropped.

Platform/lifecycle-scoped capabilities — `rebuild_handle()`, `post_frame_handle()`,
`text_input_handle()`, `focus_manager()` — are acquired only while an `Element` is transitioning
through `init_state`/`did_change_dependencies`, never from inside `build`/`perform_layout`/`paint`.
They live on `LifecycleContext: BuildContext`, which only those two hooks receive, so acquiring
one in `build` is a compile error (ADR-0078). See
[View, Element, RenderObject](view-element-render.md) for `BuildContext` itself.
