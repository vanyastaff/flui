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
Flutter's contract: `BuildOwner.buildScope` re-sorts `_dirtyElements` on
every iteration, and a `markNeedsBuild` mid-build is absorbed by
`Element`'s own `if (dirty) return` guard — FLUI's inbox-based external
scheduling had no equivalent per-iteration re-entry point.

**Choice:** absorb per pop, bounded by a per-*frame* budget
(`BuildOwner::mid_drain_absorbs_left`, reset to `MAX_MID_DRAIN_ABSORBS = 16`
at every `build_scope` entry and shared by every drain the frame runs —
including the layout-builder fixpoint's own `drain_prepared_build_target`
calls, since none of those re-enters `build_scope` itself). The budget
charges only a RE-ENTRY — a `Vacant` absorb for an id already recorded in
`BuildOwner::built_this_frame` (it already built once this frame, so this
landing is the SAME element rescheduling itself after its own build, not an
independent first-time notification) — never a first-time absorb, however
many independent elements get notified in one frame: each id can be a
first-time absorb at most once, so that case is inherently finite (a page
whose N unrelated parents each retarget an implicit animation in one frame
performs N legitimate first-time absorbs, none of them charged). On
exhaustion, the id is left in the inbox for the next `build_scope` (the
existing `has_dirty_elements` gate already schedules that frame) and a
`tracing::warn!` fires once per streak, re-arming only after a frame that
ends with budget left — there is no frame-complete hook on `BuildOwner`, so
the re-arm check runs retroactively at the *next* `build_scope` entry.

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

**Divergence 2 (a self-rescheduler keeps rebuilding; Flutter drops it):**
Flutter's `if (dirty) return` in `markNeedsBuild` silently drops a
self-`setState` issued *during* the element's own build — the dirty flag is
already set, so the second call is a no-op, and the element builds once for
both causes combined. FLUI cannot tell "during my own build" from "after it,
before the next pop" apart from a synchronous `schedule` call alone: by the
time the drain gets back around to absorbing the inbox, the building
element's build has already returned and its `dirty_reasons` entry has
already been removed (ordinary post-build cleanup, not something this
change added), so a same-id re-entry looks identical to a legitimately new
schedule from any other element. FLUI therefore rebuilds a
self-rescheduler once per re-entry, up to `MAX_MID_DRAIN_ABSORBS` — Flutter's
tighter contract would need `RebuildHandle`'s inbox entry to also record
"was this scheduled during a build the current drain has not yet
reconciled," which is out of scope for this change.

**Stale-id hazard (documented, not solved):** `ElementId` slots are reused
immediately (a remove-then-insert in one reconcile pass yields the same
id). A `RebuildHandle` captured for an element unmounted during the SAME
parent's phase-2 reconcile that also mounts its replacement can therefore
name the replacement's id by coincidence. Same-drain absorption makes this
land in the same frame that produced it, where it previously would have
surfaced (if at all) a frame later — same-drain absorption makes the hazard
likelier to be observed, not new. A generation-carrying `ElementId` that
would let the drain detect a stale handle is out of scope for this change.
