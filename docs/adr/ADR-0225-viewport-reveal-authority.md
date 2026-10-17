# ADR-0225: Viewport reveal follows registered semantics ancestry

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0168](ADR-0168-descendant-reveal-through-scroll-ancestors.md)

## Context

`ViewportOffset` required an `allow_implicit_scrolling` answer, and the scrollable
offset and fill-viewport sliver stored configurable flags. No production path
consulted them. They suggested a reveal policy that callers could configure but
the framework did not implement. Applying such a flag to cache clipping would
also contradict the existing descendant-reveal contract.

## Decision

Remove the unused trait method, stored flags and setters. A viewport offset owns
scroll position and transactional layout proposals. A fill-viewport sliver owns
child geometry. Neither independently authorizes accessibility actions.

ADR-0168 remains authoritative: a real registered `ScrollIntoView` ancestor keeps
materialized descendants addressable beyond `SemanticsClip::ScrollCache`. Authored
bounds, hidden state, exclusion and occlusion retain their existing meanings.
Nested reveal walks the actual registered ancestry, inner viewport first.

This removes an inert surface without introducing another policy or changing
runtime behavior. A future configurable reveal policy needs an explicit consumer,
an action-routing contract and tests at that routing boundary.

## Migration and evidence

External `ViewportOffset` implementations remove the method. Callers remove
`set_allow_implicit_scrolling` and corresponding getters; those calls had no
behavioral effect. The public widget cases
`show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse` and
`show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry` pin the
existing addressability and nested-delivery contracts.
