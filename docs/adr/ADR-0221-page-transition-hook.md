# ADR-0221: Page transitions come from an ambient hook; policy lives in packages

- **Status:** Accepted
- **Date:** 2026-10-13
- **Related:** [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md), [ADR-0013](ADR-0013-render-object-attach-self-dirty-handle.md)

## Context

`PageRoute` and `PopupRoute` in `flui-widgets` default to a 300 ms jump cut. A route animates
only when its author passes `.transitions(builder)`. `flui-cupertino` does this in
`cupertino_page_route`, and `flui-material` has no page route at all, so every Material
navigation and `show_dialog` cuts.

Material's motion differs per platform: a fade-forwards transition on Android, the iOS slide
on Apple platforms, a zoom elsewhere. Apps also switch to the shared-axis and fade-through
transitions. That choice is policy, and it belongs to the design-system package. The
mechanism that lets a route pick up a transition it was not given belongs to `flui-widgets`.
Official packages depend only on `flui-sdk` (ADR-0088), so `flui-material` cannot reach
`flui-cupertino`'s slide.

## Decision

**D1 — A builder trait and an ambient hook in `flui-widgets`.**

```rust
pub trait PageTransitionBuilder {
    fn duration(&self) -> Duration;
    fn reverse_duration(&self) -> Duration { self.duration() }
    fn build(&self, ctx: &BuildContext, primary: &RouteAnimation, secondary: &RouteAnimation,
             child: BoxedView) -> BoxedView;
}
pub struct PageTransitions { .. }   // inherited view holding Rc<dyn PageTransitionBuilder>
```

`PageTransitions` has separate slots for page routes and popup routes.

**D2 — The navigator resolves the hook when it installs a route.** A route that was given no
`.transitions()` adopts the nearest `PageTransitions` builder of its kind, with its durations,
when the navigator installs it and before the push animation starts. An explicit
`.transitions()` or duration on the route always wins. A route installed with no hook in scope
keeps the jump cut.

The navigator reads a hook builder's durations when it takes the hook (`init_state`,
`did_change_dependencies`), with no navigator lock held. Install runs inside the locked history
flush and calls no builder method there, so a builder may query the navigator it serves.

A route adopts its builder once. A theme change after the push does not swap the transition of
a route already on the stack. Changing it mid-flight would replace a running transition with
one at a different progress mapping.

**D3 — Policy lives in packages.** `flui-material` maps `TargetPlatform` to a `PageTransition`
in its theme and installs the matching `PageTransitions` in `MaterialApp`. `flui-cupertino`
builds its route through the same trait.

**D4 — Shared transitions move down, not sideways.** A transition used by more than one package
lives in `flui-widgets`. The iOS parallax slide moves out of `flui-cupertino` into
`flui-widgets`. `flui-cupertino`'s route and Material's Apple-platform default both use that
single copy. Material-only transitions (fade-forwards, zoom, shared axis, fade through) live in
`flui-material`.

**D5 — Reduced motion settles.** The route animation is a controller run like any other, so
under `MotionPolicy::Reduce` it settles through the `Vsync` walk. Builders do not check the
policy themselves; one that branches on it must still produce the settled frame.

New items that packages reach go through `flui-sdk` per ADR-0088 §4.

## Consequences

- An app without a design-system package keeps the jump cut it has today.
- A route can be pushed before its navigator exists. Its transition is fixed at install, not at
  construction.
- Custom transitions implement one trait, whether they are passed to a route directly or
  installed for a subtree.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| Resolve the theme at route construction | A route has no `BuildContext` when built, and most are built in event handlers. |
| Re-resolve on every build | A theme change would swap a running transition mid-flight. |
| Material copies the iOS slide | Two copies drift. Shared code moves down a layer. |
| A process-wide transition registry | State belongs to a UI runtime or a subtree, never to the process. |
