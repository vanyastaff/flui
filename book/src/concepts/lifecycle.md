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

This is the same four-state shape as Flutter's own `_ElementLifecycle` (`initial` / `active` /
`inactive` / `defunct`) — a case where FLUI follows the Flutter contract directly rather than
diverging from it, per AGENTS.md's Prime Directive: name the contract, and the behavior is proven
by test rather than assumed from the name alone.

Platform/lifecycle-scoped `BuildContext` capabilities — `rebuild_handle()`, `post_frame_handle()`,
`text_input_handle()`, `focus_manager()` — are acquired only while an `Element` is transitioning
through `init_state`/`did_change_dependencies`, never from inside `build`/`perform_layout`/`paint`;
see AGENTS.md's Architecture Constraints table (port-check trigger #22) for why, and
[View, Element, RenderObject](view-element-render.md) for where those methods live on
`BuildContext`.
