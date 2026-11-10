# ADR-0321: A motion gate reads what the window shows each frame

- **Status:** Implemented (2026-11-10)
- **Date:** 2026-11-10
- **Related:** [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) (the gate is part of the
  widget-level kit, time and frames come from it); [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)
  (a recorded frame is `UiRuntime::pump`); [ADR-0098](ADR-0098-owned-f64-geometry-values.md)
  (samples are window-logical, never mixed with device pixels);
  [ADR-0276](ADR-0276-a-follower-takes-its-leaders-full-transform-and-may-hide-with-its-clips.md)
  (a follower's placement is part of what the window shows);
  [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) (layer ids
  live one frame, so the layer tree cannot carry identity across frames)
- **Sources:** `anomalyco/rune@70eff81165e9854834a75f17753edb6c96ab2ab7`
  `rune::motion_gate::check::RULES` (line 38 of the `rune` crate's `motion_gate/check.rs`), the
  rule table of a gate of the same family over keyed elements

## Context and problem statement

Every implicit animation, transition and route in `flui-widgets` is tested by sampling one
property at chosen frames (`crates/flui-widgets/tests/implicit_animations.rs`). Nothing checks the
whole window over a run: a widget that moves, fades, mounts or unmounts at once, a spring that
passes its target or never stops, or a window that keeps asking for frames after the motion ends
passes every such test. These are the defects a person sees first. The question: what does a
test record each frame, and how does it decide that the motion it recorded is what a person would
call smooth?

## Decision drivers

- The record is what the window shows: transforms, clips, followers and group opacity applied the
  way paint applies them, not the laid-out tree alone.
- One frame of the record is one `UiRuntime::pump` on the kit's manual clock; no wall time.
- A node keeps its identity across frames, and a remount is a new identity.
- A run of the same tree names the same node the same way, so an exception can be written down.
- A finding is reported once, on the node that moved, not on each wrapper that moved with it.
- Every exception states its reason, and an exception that no longer matches fails.
- No new crate, and test support stays test support (ADR-0243): production code gains only pure
  declaration hooks the walk reads, never a recording path.

## Considered options

1. **The render tree, read the way paint reads it.**
2. **The layer tree of the submitted scene.**
3. **The semantics tree.**

## Decision

Chosen option: 1, because it is the only source that has both an identity that lasts across
frames and every property paint applies.

§1. **The sample.** `flui-rendering` (feature `testing`) gives
`PipelineOwner::presented_nodes(&self) -> Vec<PresentedNode>`:

```rust
#[non_exhaustive]
pub enum PresentedKind { Box, Sliver }

#[non_exhaustive]
pub struct PresentedNode {
    pub id: RenderId,              // generational: a remount is a new id
    pub parent: Option<RenderId>,
    pub slot: usize,               // the child slot under the parent
    pub name: &'static str,        // the render object's type name
    pub kind: PresentedKind,
    pub rect: Rect,                // the laid-out box as drawn, in window-logical points
    pub visible: Option<Rect>,     // what every ancestor clip and the window leave of it
    pub opacity: f64,              // its own, every ancestor's and their paint scopes', multiplied
}
```

The walk keeps the paint walk's gates (a hidden follower, `skip_paint`, a node that needs layout,
an invisible sliver, a child its parent left out of the last paint: the children each parent
painted are what it visits, not the semantics predicate, which drops a page a popup covers but
still shows), maps a node's own box through `RenderObject::drawn_transform` (the transform its
`PaintEffects` declare, or one its `paint` applies to its own drawing as an offset, as a
`Container` does with a translation), composes each step to a child with
`RenderObject::apply_paint_transform` and the follower transform the last composite resolved,
intersects each parent's `describe_approximate_paint_clip`, and multiplies
`PaintEffects::opacity` with the group opacity of the scopes the last paint drew each child in (a
flow delegate's per-child opacity). Paint stores that list per parent as `PaintedChild { slot,
opacity }` only when it differs from every child in order at opacity 1. Each hook is pure in
`(self, size)` and runs no user code, so the walk cannot re-enter or panic part-way through a
tree. A node whose transform is not finite, or
whose corners reach the perspective horizon, is left out with its subtree, so no non-finite
geometry is published.
Geometry stays window-logical (ADR-0098); a caller that needs device pixels multiplies by the
window's scale factor.

§2. **The rules and the charge.** `flui_testing::motion` records one `Frame` per pump
(`time` from the kit clock since the run started, `input`, `direct`, `wants_frame` from
`UiRuntime::time_to_next_frame() == Some(ZERO)`, and a `Sample` per presented node with its
`locator`). An input is recorded with the zero-time frame it drives, so its effect lands on its
own frame. The run's user code (the input closure, the build, layout and paint a pump drives)
runs before a frame is sampled; a panic in it propagates out of `MotionRun` to the test, which
fails at that frame, and no partial frame is recorded. An edge, below, is an edge of the part the window shows where the node is drawn, so a
change confined to clipped-away geometry moves nothing. `check_all(&Recording, &MotionConfig) ->
Vec<Finding>` applies:

| Rule | Fires when |
|------|------------|
| `pop-in` | a node mounts after frame 0 drawn at `pop_opacity` or more, with nothing about it changing on its next frame |
| `pop-out` | a node unmounts while drawn at `pop_opacity` or more |
| `jump` | an edge moves `jump` or more in one frame between frames where it holds still, outside a `direct` frame |
| `fade-snap` | a drawn node's opacity steps by `fade_snap` or more in one frame |
| `flicker` | opacity falls by `flicker_dip` or more from any peak within `flicker_window` before it and climbs back to that peak within the window, in one step or many; a frame the node is left out of, or drawn wholly clipped in, counts as opacity 0 |
| `overshoot` | after the last input an edge that travels `still` or more passes where it comes to rest by more than `overshoot` in a frame that shows it |
| `never-settles` | something shown still moves, fades or changes its visible part in the last `settle_window`, which lies after the last input |
| `wakes-after-end` | a frame is still asked for `wake_after_end` after the last frame that changed what the window shows (a hidden node, or the part of a box past its clip, or hidden nodes mounting and unmounting, changes nothing; each frame is compared with the last one that changed, so a slow drift adds up) |

Frame 0 is the settled start: what it draws loaded with the view and is not an entrance. The
thresholds live in one `#[non_exhaustive] MotionConfig` with defaults. `check` is
`charge_to_outermost(check_all(..), &recording)`: a finding is dropped when the node's parent, in
that frame, shows a finding of the same rule in the same frame.

§3. **The allowlist.** `gate(&Recording, &MotionConfig, &[Allow]) -> Result<(), MotionReport>`.
An `Allow` names a rule, a part of a node's `locator` (render type names from the root, each with
its child slot when it is not `0`: stable between runs of one tree, unlike a `RenderId`), a reason
and a `max`. The report fails on a finding no row covers, a row that covers more than its `max`, a
row that matches nothing, a row without a locator or a reason, and a `MotionConfig` threshold a
rule cannot compare against (a NaN or negative distance, an opacity outside `0.0..=1.0`), which
would otherwise turn its rule off without a word. `assert_smooth` is `gate` with
the default config, panicking with the report.

### Consequences

- Good, because a test of any widget can now say "this change is smooth" in three lines, and the
  rules catch the defects a sampled property cannot (a sibling jumping, a node popping in beside
  the one that animated).
- Good, because the walk reuses the paint walk's composition, so a new render object that declares
  its effects through `PaintEffects` and its clip through `describe_approximate_paint_clip` is
  covered without change.
- Bad, because the sample reads hooks, not the scene: a render object that clipped or faded by
  drawing (a shader mask over its own picture) without declaring it is sampled as fully shown.
- Bad, because a render object that paints nothing (a layout wrapper) is sampled like one that
  does; the charge to the outermost node keeps the report short, but a wrapper that moves alone is
  reported.
- Neutral, because the sample is a `Vec` per frame: test support, recorded at frame rates a test
  drives, not on a production path.

### Confirmation

- `crates/flui-testing/tests/motion_gate.rs`, run as the table `motion_gate_rules`: each rule fires
  on the motion it names and stays quiet on the animated version of the same change
  (`a_padding_change_jumps_unless_it_animates`, `an_opacity_change_snaps_unless_it_fades`,
  `a_node_that_appears_at_once_pops_in_and_one_that_enters_does_not`,
  `a_node_that_vanishes_pops_out`, `an_opacity_dip_that_comes_back_flickers`,
  `a_blink_through_opacity_zero_flickers`,
  `a_bouncy_spring_overshoots_and_a_critical_one_does_not`,
  `motion_still_running_at_the_end_never_settles`,
  `frames_asked_for_after_the_motion_ends_wake_after_end`,
  `wakes_after_end_reads_what_the_window_shows`).
- `a_finding_is_charged_to_the_outermost_node_that_shows_it` pins §2's charge;
  `the_allowlist_covers_with_a_reason_and_refuses_rows_that_match_nothing` pins §3, the refused
  config included; `presented_nodes_compose_opacity_and_clip` pins §1's composition and
  `a_page_under_a_popup_stays_presented` its membership by paint;
  `a_drag_step_moves_on_its_direct_frame` pins that an input is recorded with the frame it drives,
  and `a_finding_names_the_node_where_it_was` that a finding's locator is the one of its frame.

## Pros and cons of the options

### 1. The render tree, read the way paint reads it

- Good, because `RenderId` is generational: identity lasts across frames and a remount is visible.
- Good, because transforms, clips, followers and group opacity are the hooks paint itself reads.
- Bad, because effects drawn without being declared are invisible to it.

### 2. The layer tree of the submitted scene

- Good, because it is exactly what the raster side draws.
- Bad, because `LayerId` is minted per tree and lives one frame (ADR-0304), and only a repaint
  boundary's layer carries a `RenderId`: nothing pairs most content across frames.

### 3. The semantics tree

- Good, because its nodes have stable ids and the labels a locator reads.
- Bad, because it has no opacity, merges and drops nodes, and so cannot see a fade, a pop or most
  of the window.

## More information

The rule set and thresholds follow the same family of checks in Rune's motion gate (the
`RULES` table cited in the header), read against FLUI's own trees; FLUI's identity, locator and charge are its own. Revisit when the
retained scene (ADR-0304) gives layers an identity that lasts across frames: sampling the scene
would then cover effects drawn without being declared. A rule that reads the frame's build report
(a scroll that rebuilds) is the next addition the frame sample can carry.
