# ADR-0235: The refresh indicator comes from an ambient style; the design system installs it

- **Status:** Accepted
- **Date:** 2026-10-17
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
in the indicator's slot at the top of the content; `None` shows nothing. `progress` is the pull
as a share of the refresh threshold, in `[0, 1]`: a positive pull past a zero threshold is `1`,
and a NaN or negative share is `0`, so a builder never sees a non-finite value.

**D2 — The default is today's.** Without a style, an `ActivityIndicator` shows while refreshing
and nothing while pulling. A pull then changes only the pull distance and rebuilds nothing.

**D3 — The pull rebuilds only under a style.** The overlay rebuilds on a phase flip always and,
while its last build found a style, on every pull change, so the style's indicator follows the
pull. The content and the gesture handlers never rebuild for either.

**D4 — Policy lives in packages.** `flui-material`'s `MaterialApp` installs a style that shows a
determinate `CircularProgressIndicator` while pulling and an indeterminate one while
refreshing. An Expressive theme switches it to `LoadingIndicator` when that indicator lands.

**D5 — Reduced motion is the indicator's own.** The style does not branch on the motion policy;
the indicator it returns settles or stands still under `MotionPolicy::Reduce` as it would
anywhere (ADR-0192).

New items that packages reach go through `flui-sdk` per ADR-0088 §4.

## Consequences

- An app without a design-system package keeps the spinner it has today, and pulling still
  rebuilds nothing.
- A custom indicator for a subtree is one `RefreshIndicatorStyle`, the same mechanism the
  design system uses.
- The builder's identity is compared by pointer: a style rebuilt with a new closure (a theme
  change in `MaterialApp`) rebuilds the indicators below once.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| An `indicator_builder` on each `RefreshIndicator` | Every call site repeats the design system's choice, and a Material app cannot change it in one place. |
| Moving `CircularProgressIndicator` down into `flui-widgets` | It is Material's component, with Material tokens; the mechanism belongs in the layer, the look in the package (ADR-0221 D3). |
