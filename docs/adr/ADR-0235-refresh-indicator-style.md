# ADR-0235: The refresh indicator comes from an ambient style; the design system installs it

- **Status:** Accepted
- **Date:** 2026-10-17
- **Amended:** 2026-10-18, accepted by the owner — D1–D3 on the nested-scroll `RefreshIndicator`
  (ADR-0209). The indicator's slot is a band at the pull's leading edge (the top of a vertical
  list, the reading start of a horizontal one) that slides in with the pull, so the overlay
  rebuilds on every pull change, with or without a style; the content and the gesture handlers
  never rebuild. Without a style the band holds the `ActivityIndicator` while pulling and while
  refreshing. A threshold that is not finite and positive is refused where it is set, so
  `progress` is always the pull over a positive threshold, capped at `1`. This replaces the
  earlier rule that a pull without a style rebuilds nothing.
- **Related:** [ADR-0221](ADR-0221-page-transition-hook.md), [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md), [ADR-0192](ADR-0192-accessible-loading-motion.md)

## Context

`RefreshIndicator` in `flui-widgets` drew a fixed `ActivityIndicator` in a fixed blue while
refreshing and nothing while the user pulled. A Material app needs Material's circular
indicator, determinate as the pull grows and indeterminate while refreshing, and Material 3
Expressive apps need the `LoadingIndicator` shape morph. Both live in `flui-material`, which
`flui-widgets` cannot depend on, and an app should not have to pass an indicator to every
`RefreshIndicator` it builds.

## Decision

**D1 — An ambient style in `flui-widgets`.**

```rust
pub enum RefreshIndicatorPhase { Pulling { progress: f64 }, Refreshing }   // #[non_exhaustive]
pub struct RefreshIndicatorStyle { .. }                                     // inherited view
impl RefreshIndicatorStyle {
    pub fn new(builder: impl Fn(RefreshIndicatorPhase) -> Option<BoxedView> + 'static,
               child: impl IntoView) -> Self;
}
```

Every `RefreshIndicator` below the nearest style shows what the builder returns for its phase,
in the indicator's band at the leading edge of the content; `None` shows nothing. `progress` is
the pull as a share of the refresh threshold, in `[0, 1]`: a NaN or negative share is `0`, so a
builder never sees a non-finite value. A threshold that is not finite and positive is refused
where it is set (the default stands), so the share is always the pull over a positive
threshold.

**D2 — The default is a spinner in the band.** Without a style, the band holds an
`ActivityIndicator` while pulling and while refreshing, and nothing while idle.

**D3 — The pull rebuilds the overlay, with or without a style.** The band slides in as the pull
nears the threshold and rests fully in while refreshing, so the overlay rebuilds on every pull
change and on every phase flip. The content and the gesture handlers never rebuild for either.
With the sliding band, rebuilding while pulling is the honest behaviour: the band's position
is the pull. The earlier rule, that a pull without a style rebuilds nothing, was an
optimisation for the previous implementation, which drew nothing while pulling.

**D4 — Policy lives in packages.** `flui-material`'s `MaterialApp` installs a style that shows a
determinate `CircularProgressIndicator` while pulling and an indeterminate one while
refreshing. A theme with `MaterialStyle::Expressive` switches it to `LoadingIndicator`.

**D5 — Reduced motion is the indicator's own.** The style does not branch on the motion policy;
the indicator it returns settles or stands still under `MotionPolicy::Reduce` as it would
anywhere (ADR-0192).

New items that packages reach go through `flui-sdk` per ADR-0088 §4.

## Consequences

- An app without a design-system package keeps the spinner, which now also follows the pull
  in its sliding band; only the overlay rebuilds for it.
- A custom indicator for a subtree is one `RefreshIndicatorStyle`, the same mechanism the
  design system uses.
- The builder's identity is compared by pointer: a style rebuilt with a new closure (a theme
  change in `MaterialApp`) rebuilds the indicators below once.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| An `indicator_builder` on each `RefreshIndicator` | Every call site repeats the design system's choice, and a Material app cannot change it in one place. |
| Moving `CircularProgressIndicator` down into `flui-widgets` | It is Material's component, with Material tokens; the mechanism belongs in the layer, the look in the package (ADR-0221 D3). |
