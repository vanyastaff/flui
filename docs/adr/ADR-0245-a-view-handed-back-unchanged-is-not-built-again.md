# ADR-0245: A view handed back unchanged is not built again

- **Status:** Accepted (2026-10-19)
- **Date:** 2026-10-18
- **Related:** [ADR-0041](ADR-0041-workspace-topology-contract.md),
  [ADR-0074](ADR-0074-realm-scoped-signals.md),
  [FOUNDATIONS](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/FOUNDATIONS.md) Part II item 4

## Context

Every widget that wraps a child keeps it in a `BoxedView` and returns a clone
of it from `build`. The element tree had no way to see that the clone was the
view the child element already held: `BoxedView` copied the erased view on
every clone, so each rebuild of a wrapper updated its child, marked it dirty
and built it again, and so on down the whole subtree.

`Focus` and `FocusScope` rebuild on every focus edge so that the descendants
reading their state stay current (the `FocusParent` value they publish carries a revision for
exactly that). The first focus gain of an app, which a `Scrollable`'s drag
start causes by focusing the scroll view, flips `has_focus` on every enclosing
focus node, and each rebuilt the app's whole subtree. A root that made a
`ScrollController` in `build` handed its `Scrollable` a different position in
the middle of the drag, which cancelled the contact: [`examples/sliver_demo.rs`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/examples/sliver_demo.rs)
froze in the live smoke. The same cascade rebuilds everything under any
`Navigator`, `Theme`, `MediaQuery` or animation wrapper that rebuilds for its
own reasons.

`View::should_skip_rebuild` and `Memo<V>` skip a rebuild when contents compare
equal, but only for views that implement `PartialEq`, which a view holding a
callback or a child cannot. A wrapper's child is exactly that.

## Decision

`BoxedView` is a shared view: it holds an `Rc<dyn View>` and cloning shares it.
The allocation is the view's identity. Boxing a `BoxedView` returns it as it
is (`BoxedView::new`, `ViewExt::boxed`, `From<Box<dyn View>>`), so a view keeps
its identity through however many wrappers pass it along. The field is private;
`BoxedView::new` and `BoxedView::as_view` replace constructing and reading it.

An element keeps its configuration in the shared view itself: `View::create_element`
takes the view as `Rc<Self>`, so `BoxedView::create_element` hands the element
its allocation, and an update that takes a view from a `BoxedView` takes that
allocation. A view offered by reference alone is copied into an allocation of
its own. The element holds no second copy of the view, and since it holds the
allocation, no other view can take its address. An update offering the shared
view the element already holds returns before replacing or marking anything:
no `did_update_view`, no render-object update, no dirty flag. What changed for
the element reaches it by its own schedule (a state change, an inherited value,
a signal).

`Child` and `Children` lend their `BoxedView`s as views, not the views inside
them, so the reconciler sees the identity.

The check runs ahead of `should_skip_rebuild`, which is unchanged and still
the opt-in for views that compare equal without being the same value.

## Consequences

A widget that rebuilds for its own reasons rebuilds what it builds, and the
child it was given is rebuilt only if it depends on what changed. A `build`
that reads state which is neither an inherited value, a signal nor its own
configuration no longer refreshes when an ancestor rebuilds; it was relying on
a cascade nothing promised.

`BoxedView(Box<dyn View>)` and its public field are gone. Code that built one
with `BoxedView(Box::new(view))` calls `BoxedView::new(view)`; code that read
`.0` calls `as_view()` or uses the `BoxedView` as the view it is.

Tests: `the_first_focus_gain_does_not_rebuild_the_app_under_a_drag` pins the
end-to-end case, and
`a_child_handed_back_unchanged_is_built_only_when_something_it_reads_changes`
pins the element side: an unchanged offer builds nothing, a fresh view still
does, and an unchanged child that depends on what changed still does.
