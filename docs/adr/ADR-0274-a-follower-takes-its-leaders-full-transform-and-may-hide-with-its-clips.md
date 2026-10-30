# ADR-0274: A follower takes its leader's full transform and may hide with its leader's clips

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Related:** [ADR-0232](ADR-0232-annotated-regions-and-overlay-style.md) (the annotation lookup
  places a follower through the same resolution)

## Context and problem statement

A `FollowerLayer` places content (selection handles, tooltips, menus) at the `LeaderLayer` that
shares its `LayerLink`, wherever that leader sits in the layer tree. The resolver summed each
layer's translation along the leader's and the follower's ancestor chains, so:

| Case | Result |
|------|--------|
| A `TransformLayer` with a scale or rotation between the two | the follower took only its translation and drifted from the leader |
| A leader painted inside another follower (a submenu in a menu) | the outer follower contributed zero, so the inner one was misplaced |
| A scroll viewport's clip around the leader, the follower in an `Overlay` | the clip sits below the common ancestor, so the follower kept painting and taking hits after its leader scrolled out |

Seven consumers read the result: the GPU walk, the damage differ, the backdrop damage pass, the
annotation lookup, the hit-test side table and the input-region walk in `flui-rendering`. How is a
follower placed, and when does a clipped leader hide it?

## Decision drivers

- One resolution, read by every consumer, so pixels, hits and input regions agree.
- No geometry that is not finite is published; a degenerate chain paints nothing rather than a
  fabricated position.
- Tooltips and menus escape clips by design; selection handles must not.
- The frame path is synchronous: resolution reads the built tree, with no feedback channel from
  the engine.

## Considered options

1. **The full transform, plus a per-follower clip policy tested at the leader's anchor point.**
   - Good: the reference's `FollowerLayer._establishTransform` rule, so a follower lands on its
     leader under any invertible chain.
   - Good: the clip test is exact for every clip shape, since it asks whether one point is inside.
   - Bad: a follower now scales and rotates with its leader.
2. **Map only the anchor point through the chains and translate the follower.**
   - Good: a menu keeps its own size under a scaled leader.
   - Bad: under a rotation the follower sits at the right point with the wrong orientation, and it
     is a second rule beside the reference's.
3. **Clip the follower to the leader's clips.**
   - Good: a follower cut off at the viewport edge, like the leader.
   - Bad: the renderer would push a clip expressed in another subtree's coordinates; no widget
     needs it yet.
4. **A visibility signal each render object publishes for its own followers** (the reference's
   `selectionStartInViewport`).
   - Good: no layer-level change.
   - Bad: every follower kind reimplements it, and it lags a frame behind the composite that
     already has both chains.

## Decision

Chosen option: 1, because it is the one rule that places a follower correctly under every chain,
and the anchor point is what a follower is attached to.

§1 `flui_layer::resolve_follower_transform(&LayerTree, LayerId) -> Option<Matrix4>` is the one
resolution. With `L` the product of the leader's chain from the common ancestor (exclusive) down to
the leader, and `F` the same for the follower's parent, the result is
`F⁻¹ · L · translate(FollowerLayer::linked_offset(leader_size))`. A chain layer contributes its
matrix (`Transform`), its own resolved transform (a nested `Follower`), or its
`Layer::local_translation`. `None` means nothing is painted or hit there: unlinked with
`show_when_unlinked == false`, a singular or non-finite transform on either chain, a hidden nested
follower, more than `MAX_FOLLOWER_NESTING` followers on the chains (a debug build warns once per
resolution), or §2.

§2 `FollowerLayer` carries `#[non_exhaustive] enum LeaderClip { Escape, Hide }`, default `Escape`.
Under `Hide` the leader's anchor point is mapped up the leader's chain, and the follower resolves
to `None` while that point lies outside a clip layer with `Clip != None` on it. The clip shapes are
closed. Clips at or above the common ancestor already clip the follower and are not consulted.
`RenderFollowerLayer::with_leader_clip` sets it, and `PaintCx::with_follower` takes a
`FollowerLayer` value.

### Consequences

- Good, because a follower lands on its leader under scale, rotation and nested followers.
- Good, because hit-testing and input regions inherit both rules through the hidden-follower table,
  so a hidden handle takes no drag.
- Bad, because a widget that wants an unscaled follower under a scaled leader needs a later
  policy; `LeaderClip`'s sibling would be a transform policy on `FollowerLayer`.
- Neutral, because `Escape` keeps the behaviour every existing follower had.

### Confirmation

`flui-layer`'s `follower_resolution` table (`tests/follower.rs`: `a_scaled_leader_scales_the_follower`,
`a_rotated_leader_rotates_the_follower`, `a_leader_inside_another_follower_resolves_through_it`,
`a_singular_transform_on_a_chain_resolves_to_nothing`,
`followers_nested_past_the_limit_resolve_to_nothing`, `a_cycle_through_leaders_resolves_to_nothing`,
`hide_follows_the_anchor_through_the_leader_clip`,
`a_clip_at_the_common_ancestor_is_not_consulted`), the `render_object_harness` rows
`harness_follower_layer_hit_tests_under_a_scaled_leader` and
`harness_follower_layer_hides_with_a_clipped_leader`, and the engine readback row
`followers_take_the_leaders_scale`.
