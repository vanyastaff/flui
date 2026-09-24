# ADR-0068: A frame's outcome is a three-state disposition, and a withheld frame is retained under a bound

- **Status:** Accepted
- **Date:** 2026-09-17
- **Related:** [ADR-0044](ADR-0044-driver-loop-hybrid.md) §7 (wall-clock wake); [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) (the fallback gate)

*A `Result<bool>` cannot say "this frame owed the screen something and did not
get it there". Collapsing that into "nothing was owed" parked the native macOS
back end on a window that had never drawn. `PresentDisposition` splits the two
answers, the realm retains the withheld one, and the retention carries its own
cap because the platform's occlusion gate does not bound it.*

## Context

`RasterBackend::render_scene` returned `Result<bool, EngineError>`: `true` if
the frame reached the screen. Every way of *not* reaching the screen collapsed
into `false` — nothing to draw, the drawable was occluded, the surface had
been released — and the frame tail treated all of them the same way: the
frame is finished, the loop may park.

For one of those cases that is wrong, and the failure is total. When the
backend is handed content it cannot put on screen, the pipeline has already
consumed the work that produced the scene. Parking there leaves the loop with
nothing to wake it (no scheduled frame) and nothing to draw if it did (the
scene was taken). The window never paints again.

This is reachable on ordinary cold start. Under the native AppKit back end,
`sliver_demo` cold starts presented 0 or 1 frames in ~8 s: the surface's first
drawable reported `Occluded`, the frame was classified as finished, and the
process parked at 0% CPU on a window that had never drawn. With the retention
below (and later with the cap of §4 as well) the same runs presented hundreds
of frames.

## Decision

### 1. `PresentDisposition` replaces the `bool`

```rust
pub enum PresentDisposition { Presented, NoDamage, NotShown }
```

- `Presented` — the frame reached the screen.
- `NoDamage` — nothing was owed; the loop may park.
- `NotShown` — content *was* owed and was not shown. The caller owes this
  frame a retry.

`classify_frame(had_damage, acquired_surface)` holds the mapping
(`(false, _) => NoDamage`, `(true, true) => Presented`, `(true, false) =>
NotShown`) as a free function, so the table is testable without a window: the
arm that feeds it `acquired_surface = false` needs a windowed surface with an
occluded drawable, which no test in `flui-engine` can construct.

### 2. The enum is exhaustive, not `#[non_exhaustive]`

Every sibling protocol type in this workspace is `#[non_exhaustive]`; this one
deliberately is not. The defect was a *silent conflation* of two answers, and
`#[non_exhaustive]` invites exactly the wildcard arm that reconstitutes it — a
future state folded into "nothing owed". Exhaustiveness paid for itself
immediately: adding the variant made the compiler name the one `SubmitVerdict`
match site that had to classify it (`raster_lane.rs`).

### 3. `NotShown` is retained, and the retention is two-part

The frame tail commits the tree but does not finish the frame:

```rust
retry_needed = true;          // wake_frame()
retry_needs_repaint = true;   // mark_needs_full_repaint_for(producer)
```

Both halves are required, for the reason issue #637 records for the
submit-failure arms. `wake_frame()` alone reopens the segment gate but leaves
`PipelineOwner` with nothing dirty; the retry pump produces `Idle`, never
reaches `render_scene`, and parks — the frame is lost exactly as before.
`mark_needs_full_repaint_for` is what gives the retry real work.

`NoDamage` clears: nothing was owed, so nothing is retained. That is the other
half of the contract, and without it "retain everything" would satisfy the
retention test while repainting an idle app at the fallback pace forever.

### 4. The retention is bounded by count, and the platform does not bound it

The retry re-dirties the presentation on every attempt, so an unbounded
retention is a self-sustaining repaint loop paced by the runner's fallback
deadline (~9.5 ms, ~105 Hz).

It would be convenient if the platform's own occlusion gate bounded it. It
does not. A window AppKit reports as occluded *does* stop reaching this arm —
`windowDidChangeOcclusionState:` → `WindowVisibility` →
`AppLifecycleState::Hidden` → `frames_enabled == false` → `wake_action`
short-circuits to `PumpAsync` — but that gate keys off AppKit's
`occlusionState`, while what withdraws the drawable is the *swapchain's* own
availability. **Measured on cold start, they disagree**: `occlusionState`
reported the window visible throughout the ~132 ms (12 consecutive withheld
frames) the drawable was unavailable. Wherever
they disagree and stay disagreeing — a window on an inactive Space, a display
asleep — the lifecycle gate never engages and the loop is unbounded.

So the cap lives in the frame tail: `MAX_NOT_SHOWN_RETRIES = 128`, with the
per-presentation streak on `PresentationState` (`record_frame_withheld` /
`clear_not_shown_streak`). Past the cap the frame parks instead of re-arming.

The value is chosen against measurement: the withheld cold-start transient ran
at most 12 consecutive attempts (~132 ms), so 128 sits ~10× above the worst
observed (~1.2 s at the same pace). The two error directions are asymmetric, so
the generous side is the right one: too large costs one bounded burst of wasted
frames on a surface that never returns, too small reintroduces the blank window
this exists to fix. In ordinary runs the cap never engages — a backstop against
the pathological case, not a limit the ordinary transient touches.

Exhausting the budget is not permanent. The streak is cleared by any frame
that ends otherwise, so it bounds a *continuous* withdrawal rather than
disabling retention; and a real platform event still dirties the pipeline and
produces an attempt of its own. Recovery does not depend on the counter being
reset by hand.

## Alternatives considered

- **`Result<bool>` plus a separate `fn was_withheld()` query.** Rejected: two
  facts that must be read together, with no guarantee the second is consulted.
  The conflation being fixed was precisely a caller reading only the first.
- **`#[non_exhaustive]`.** Rejected — §2.
- **No cap, relying on the platform's occlusion gate.** Rejected on the
  measurement above; it is the reading this ADR exists to correct.
- **A cap measured in wall-clock time rather than attempts.** Equivalent at
  the fallback pace and would need a clock inside the frame tail; the attempt
  count is the same quantity one layer down.

## Consequences

**Accepted cost.** A surface that stays unavailable while frames are enabled
repaints at the fallback deadline's pace for up to `MAX_NOT_SHOWN_RETRIES`
attempts before parking — the same steady-state trade the
`SubmitVerdict::SurfaceStale` arm already documents and accepts. In the
measured runs the loop is never observed in steady state: the withheld streak
occurs once, at cold start, and does not recur.

The cold-start measurements bound the transient but do not exercise the
unbounded loop (the demo only repaints while pointer events arrive), so the cap
is justified by the mechanism and carries its own tests.

**Residual.** The consumer half is covered; the producer is not. What is
executed end to end is the consumer half — a backend reporting `NotShown` is
classified at the lane and retained and bounded by the realm — plus the
engine's own classification table. The wgpu arm that supplies
`acquired_surface = false` from a real surface-acquisition failure is
read-reviewed, not executed, for want of a constructible occluded drawable.
The runtime contract registry tracked this as
`frame-disposition-distinguishes-withheld-from-idle`, state `partial`.
