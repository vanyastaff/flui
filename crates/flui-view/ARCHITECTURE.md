# flui-view Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate, as required by [`docs/PORT.md`](../../docs/PORT.md) §Per-crate
`ARCHITECTURE.md` template. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.
[`UNIFIED_ELEMENT.md`](UNIFIED_ELEMENT.md) is the crate's existing element
behaviour taxonomy and remains a sibling appendix, per `docs/PORT.md`'s graft
note for this crate.

---

## Mapping decisions

### Same-drain absorption of a mid-drain external schedule (issue #1180)

**Rule:** `BuildOwner::drain_build_scope`'s heap loop absorbs the
out-of-frame external inbox (`ExternalBuildScheduler` /
`RebuildHandle::schedule`) at the top of *every* pop, not only once before
the first one. An id landing in the inbox while its own drain is still
running joins that same `build_scope` call: `Occupied` in `dirty_reasons`
merges the new causes into whatever still-live entry holds the id (on the
heap, or sitting in a deferred layout-builder scope bucket); `Vacant` inserts
it fresh, keyed by the tree's *authoritative* depth (never the depth
captured at `schedule` time), and routes it exactly as any other
newly-dirtied id would be for the current drain target — a Global id landing
during a `LayoutBuilder(scope)` drain defers to the root bucket for the next
Global pass, and the mirror lands a scope-local id in `isolated[scope]`
during a Global drain.

**Conflict:** issue #1180 — a build that calls another element's
`RebuildHandle` synchronously (a `Listenable` notifying a subscriber it
owns, the common `AnimatedBuilder`/`AnimatedView` shape) lands in the
inbox, but the OLD `drain_build_scope` only ever emptied that inbox once,
before its loop started. A schedule arriving mid-loop therefore sat until
the *next* `build_scope` call — a whole extra frame — even though the
notifying build and the notified rebuild belong to the same
user-observable action. Concretely: a `Duration::ZERO` implicit-animation
retarget snaps the controller's value synchronously in `did_update_view`,
and the dependent `AnimatedBuilder`'s listener fires in that same instant —
but used to need a second pump to actually rebuild, because its schedule
missed the retargeting build's own drain by one absorb call. This is also
Flutter's contract, precisely at 3.44.0: `BuildScope._dirtyElementIndexAfter`
re-sorts `_dirtyElements` only when a mid-flush `_scheduleBuildFor` set
`_dirtyElementsNeedsResorting` (not on every iteration unconditionally),
and a `markNeedsBuild` mid-build is absorbed by `Element`'s own
`if (dirty) return` guard — FLUI's inbox-based external scheduling had no
equivalent per-iteration re-entry point.

**Choice:** absorb per pop, bounded by a per-*frame* budget
(`BuildOwner::mid_drain_absorbs_left`, reset to `MAX_MID_DRAIN_ABSORBS = 16`
at every `build_scope` entry). The budget is genuinely per FRAME, not per
`build_scope` CALL: `build_scope` factors into the reset plus
`build_scope_impl`, and every mid-frame re-entrant caller — the
layout-builder fixpoint's own `drain_prepared_build_target` calls, and
`service_child_requests_impl`'s lazy-sliver "second build_scope" pass —
reaches `build_scope_impl` directly instead of `build_scope`, so none of
them re-runs the reset. The budget charges only a RE-ENTRY — a `Vacant`
absorb for an id already recorded in `BuildOwner::built_this_frame` (it
already completed a build in this `build_scope` call, so this landing is
some element being notified again after that build — the common case is
the SAME element rescheduling itself, but a child notifying its
already-built parent, or an A↔B ping-pong, charges identically) — never a
first-time absorb, however many independent elements get notified in one
frame: each id can be a first-time absorb at most once, so that case is
inherently finite (a page whose N unrelated parents each retarget an
implicit animation in one frame performs N legitimate first-time absorbs,
none of them charged). On exhaustion, the id is left in the inbox for the
next `build_scope` (the existing `has_dirty_elements` gate already
schedules that frame) and a `tracing::warn!` fires once per streak,
re-arming only after a frame that ends with budget left — there is no
frame-complete hook on `BuildOwner`, so the re-arm check runs retroactively
at the *next* `build_scope` entry.

**Divergence 1 (no descendant-only debug assert):** Flutter's
`Element.markNeedsBuild` debug-asserts, from inside `buildScope`, that a
mid-build `markNeedsBuild` names a descendant of the element currently
building (`_debugCurrentBuildTarget`). FLUI has no equivalent assert here.
The inbox is FLUI's only route for a listener-driven or cross-thread
rebuild — Flutter routes the identical shape through `setState`, which is
always issued by the element's own `State` object and therefore always
structurally a self-notification. A `RebuildHandle::schedule` call carries
no such structural guarantee (`RebuildHandle` is `Send + Sync`, callable
from a worker thread or from an unrelated element's build with no
relationship to whichever element the drain happens to be building), so
there is no cheap invariant to assert here; recording the gap is the honest
choice over a debug assert that would either never fire (too weak to catch
anything) or reject a legitimate cross-subtree listener (too strong).

**Divergence 2 (a re-entered element keeps rebuilding; Flutter drops it):**
Flutter's `if (dirty) return` in `markNeedsBuild` silently drops a
self-`setState` issued *during* the element's own build — the dirty flag is
already set, so the second call is a no-op, and the element builds once for
both causes combined. FLUI cannot tell "during my own build" from "after it,
before the next pop" apart from a synchronous `schedule` call alone: by the
time the drain gets back around to absorbing the inbox, the building
element's build has already returned and its `dirty_reasons` entry has
already been removed (ordinary post-build cleanup, not something this
change added), so a re-entry — a `Vacant` landing for an id that already
completed a build in this `build_scope` call — looks identical whether it
is that same element rescheduling itself, a child notifying its
already-built parent, or one half of an A↔B ping-pong. FLUI therefore
rebuilds a re-entered element once per re-entry, up to
`MAX_MID_DRAIN_ABSORBS` — Flutter's tighter contract would need
`RebuildHandle`'s inbox entry to also record "was this scheduled during a
build the current drain has not yet reconciled," which is out of scope for
this change.

**Divergence 3 (`on_build_scheduled` fires mid-drain; Flutter latches its
frame request through TWO nested guards, one at each level FLUI's
`schedule` conflates):** at 3.44.0, `BuildOwner.scheduleBuildFor` guards its
own frame-request callback with `if (!_scheduledFlushDirtyElements &&
onBuildScheduled != null)` (`framework.dart`), then calls into
`BuildScope._scheduleBuildFor`, which separately guards ITS OWN per-scope
`scheduleRebuild?.call()` with `if (!_buildScheduled && !_building)`. Every
Flutter schedule — `setState`, a `Listenable` firing, a `BuildOwner`-level
reassemble — passes through BOTH guards uniformly, since there is only one
`scheduleBuildFor` entry point. FLUI's `ExternalBuildScheduler::schedule`
has no equivalent at either level: it fires `on_build_scheduled` on every
newly-queued id regardless of whether a drain is already running (pinned by
`mid_drain_schedule_still_requests_a_frame_like_an_out_of_frame_schedule`).
Recorded as the deliberate alternative rather than built: the redundant
frame request this can cause is discarded downstream by the ordinary
dirty-state gate a wake-with-nothing-new-to-do already hits, so adding the
latch(es) would trade a real per-callsite invariant (every fresh inbox
entry asks for a frame) for a saving with no measured cost — take it up
only if a wake-count oracle ever shows the cost is real.
