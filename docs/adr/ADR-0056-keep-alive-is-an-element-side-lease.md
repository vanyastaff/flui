# ADR-0056 — Keep-alive is an element-side lease, and a parked child stays in place

- **Status:** Accepted
- **Date:** 2026-09-05
- **Issue:** #835
- **Supersedes:** nothing. Amends the eviction contract in ADR-0053 and the
  evict-before-paint sentence in `crates/flui-rendering/ARCHITECTURE.md`.
- **Depends on:** the placed-generation stamp being consulted by all four
  observation walks (#834 for paint and hit-test, #881 for semantics). Without
  the semantics half this design would ship an accessibility defect it creates —
  see [Why the stamp is a prerequisite](#why-the-stamp-is-a-prerequisite).

## Context

A lazy sliver evicts every child outside its cache band
(`SparseChildren::retain_band`). The child's element subtree is unmounted, so it
loses everything its state held: a half-typed field, a playing video, a nested
scroll offset, an expanded disclosure. `SliverMultiBoxAdaptorParentData` carried
a `keep_alive: KeepAliveParentDataMixin` field for this, but **nothing in the
workspace ever read or wrote it** — it was a shape transcribed from the
reference, not a mechanism.

Flutter's mechanism has four parts: `AutomaticKeepAliveClientMixin` on a
descendant `State`; a `KeepAliveNotification(handle)` bubbling up; a per-item
`AutomaticKeepAlive` element holding a `Map<Listenable, VoidCallback>` so N
descendants can each hold independently; and a `KeepAlive` `ParentDataWidget`
writing the flag onto the sliver's parent data, which
`RenderSliverMultiBoxAdaptor.collectGarbage` consults to move the child into
`_keepAliveBucket` instead of destroying it.

## Decision

**A keep-alive hold is an RAII lease on a presentation-scoped table, taken
through `BuildContext::keep_alive_lease()` and released by `Drop`.** A held
child is skipped by band eviction and otherwise left exactly where it is.

Three of Flutter's four parts do not transfer, each for a mechanical reason in
this tree rather than a matter of taste.

### The flag cannot live in parent data here

Every lazy item is wrapped in a `RepaintBoundary` by default
(`wrap_builder_in_repaint_boundaries`), and it wraps from *outside*, so a
`KeepAlive` `ParentDataView` written by the builder always lands *inside* that
boundary. `apply_ancestor_parent_data` stops at the first render ancestor, so
the write targets the item's own render node — which the sliver never reads, and
which was never stamped with a logical index. There is no panic and no effect.
With `repaint_boundaries(false)` the same code *works*, because the item is then
the sliver's direct child. A mechanism that is silently inert in the default
configuration and functional in the non-default one is the worst available
orientation, and no amount of documentation fixes it.

Two further reasons, independent of that one: `SliverMultiBoxAdaptorParentData::default()`
carries index `0` — a real index — so the no-existing-data branch would install
parent data that trips the duplicate-index assertion in the band walk. And FLUI
decides eviction **element-side** (`SparseChildren::retain_band`) where Flutter
decides it **render-side** (`collectGarbage`); routing an element-side decision
through the render tree and back adds a second writer to a struct the logical-index
stamp already owns.

The dead `KeepAliveParentDataMixin` and both `keep_alive` fields are deleted.
Semver-safe: absent from `docs/runtime-contract.toml`, and the `flui` facade
deliberately does not re-export `flui-rendering`.

### The channel is a lease, not a notification

FLUI already has `KeepAliveNotification` and a bubbling dispatcher. Neither is
used: `dispatch_notification` has **zero production callers** and
`on_notification` **zero production impls** — the whole seam exists only in its
own tests. Building on it would also require a four-site trait-signature change,
because `on_notification(&self, TypeId, &dyn Any) -> bool` carries no sender
identity and no `&ElementTree`, and a lease keyed by `(logical_index, holder)`
goes stale the moment `reconcile` relocates a keyed resident.

A lease is better than the reference on the point Flutter's own documentation
concedes. Flutter fuses release into `KeepAliveHandle.dispose()` precisely
because a separate `release()` gets forgotten, and still notes that a missed
release keeps the subtree alive "until the list itself is disposed". A
`#[must_use]` guard whose `Drop` releases makes that unrepresentable. N holders
are then a refcount — what Flutter's handle map emulates by hand.

**A lease names its holder and nothing else.** The sparse child it keeps alive is
resolved from the tree when eviction asks, never recorded at acquisition. That is
what makes relocation free in both directions: a held child whose logical *index*
changes under reconcile keeps its hold, and so does one whose *host* changes under
a `GlobalKey` graft from one list into another. Caching the target instead was
wrong in two ways at once — the row the holder left stayed pinned forever, and the
row it moved to stayed evictable.

It follows that a lease is issued **unconditionally**, including to an element not
currently inside a lazy sliver: it simply holds nothing there, and begins holding
if the element is later grafted into a list. Refusing would make that refusal
permanent, because `init_state` is the only guaranteed acquisition point —
`activate` and `did_update_view` receive no context, and acquiring from `build` is
forbidden — so a `GlobalKey` state mounted outside a list and later moved into one
would have no supported way to ask.

The table lives on `BuildOwner`, not in an `InheritedView` scope. Every other
`BuildContext` capability resolves from the owner (ADR-0018/0021/0030/0037), and
an inherited scope would additionally need a per-item `StatefulView` wrapper and
a config field on all three sliver widgets, because the *host* also needs to read
the table and is a render behavior that cannot read inherited data ambiently.

`keep_alive_lease` is the eighth token in `scripts/check-frame-capability-scope.sh`.
Acquired from `build` it would be re-taken on every rebuild, and the previous
lease's `Drop` would release the old hold, making a child's survival depend on
rebuild ordering. Note that `init_state` runs with the same `BuildCtx` type
`build` gets, so the rule is necessarily **static**, enforced by the script —
exactly as it already is for `text_input_handle` and `focus_manager`.

### A parked child is not moved

Flutter removes a kept-alive child from the render child list and re-adopts it
on revival. FLUI leaves it attached and simply does not lay it out: the band walk
visits `cache_first..cache_last`, so an out-of-band child is never laid out, its
placed-generation stamp goes stale, and every phase that could observe it skips
it — paint, both hit-test walks, and semantics alike.

`drop_child` was considered and rejected: `RenderTree::drop_child` leaves
`parent = None`, and the scheduler treats a parentless node as its own relayout
**root**, so a parked child that dirtied itself would lay out standalone against
stale cached constraints. Park-in-place also deletes Flutter's entire
`_keepAliveBucket` re-adoption dance rather than reimplementing it.

### Holds gate band eviction only

A hold is consulted in `retain_band` and **nowhere else**. A resident the data
source stopped producing is destroyed regardless: guarding the reconcile path
would let a held child squat on index 3 while a keyed resident relocates onto 3,
leaving two attached children stamped 3 and tripping the band walk's uniqueness
assertion. Flutter draws the identical line — `collectGarbage` consults the
flag, `removeChild` does not — which is the cross-check that this split is the
contract and not a local convenience.

### A held child reconciles like a resident

`reconcile` carries keyless out-of-band residents over without rebuilding them,
because they are about to be dropped by the band eviction that follows. A held
child is the exact opposite: it persists indefinitely. Carrying it over would
leave it rendering whatever its data said when it left the band, and would deny
it the update that might release the hold. Held children therefore reconcile
like in-band residents. This was found by a test, not by reading: a released
item was never evicted because `did_update_view` had never run on it.

## Why the stamp is a prerequisite

Before this change, an out-of-band resident was *evicted*, so the ungated
semantics walk had nothing stale to reach. Keep-alive creates the first
population of long-lived attached-but-unlaid children, which converts a latent
gap into a live defect: a screen reader announcing rows at coordinates nothing
occupies. That is why #881 blocked this work rather than merely preceding it.

This also **promotes the placed-generation stamp from defence-in-depth to the
sole mechanism** keeping parked content off screen. `crates/flui-rendering/ARCHITECTURE.md`
previously recorded that lazy slivers rely on the frame evicting out-of-band
residents before paint; that reliance is now gone for held children.

The exclusion is Flutter-faithful for exactly this case:
`visitChildrenForSemantics` explicitly skips the keep-alive bucket
(`// Do not visit children in [_keepAliveBucket]`).

## Consequences

**Good.** State survives scrolling with no ceremony at the call site — one line
in `init_state`, and the lease's lifetime is the hold's. The table is keyed by
element identity rather than by sliver or index, so the same mechanism serves
`PageView`, `TabBarView` and reorderable lists unchanged. The change deletes
code on net.

**Costs, named rather than discovered later.**

- **The parked set is unbounded and user-controlled.** Nothing caps how many
  children a page may hold. `RenderSliverList::hit_test` reverse-iterates
  `0..attached_child_count`, and the band walk rebuilds its slot map from the
  attached count each pass, so both become `O(band + parked)`.
- **A parked child that keeps animating still requests frames.** It lands in
  `run_paint`'s residue scan every frame (a `warn!`, plus its retained capture
  evicted), and `fire_need_visual_update` wakes the loop for a boundary whose
  content is not on screen. Flutter does not warn here; `flushPaint` checks
  `layer.attached` and calls `_skippedPaintingOnLayer()`.
- **Release timing during a frame phase is unspecified.** Band eviction runs
  between layout passes and once post-frame; a lease dropped from a post-frame
  callback is not observed until the next frame's eviction.
- **`ItemCount::Unknown` can strand a hold.** A count clamp that shrinks below a
  held index leaves a child the band walk never lays out and `retain_band` never
  evicts.

These are tracked as follow-ups rather than fixed here, because each is a
separate mechanism (a cap and its policy, the residue scan's classification, an
eviction-timing contract) and folding them in would make one change four.

## Alternatives rejected

| Alternative | Why |
|---|---|
| `KeepAlive` as a `ParentDataView` (the issue's own proposal, and Flutter's shape) | Silently inert in the default config; index-0 default trips the band walk's uniqueness assert; routes an element-side decision through the render tree |
| Keep the `keep_alive` parent-data field as the flag's home | Dead field, zero readers; a second writer on a struct the index stamp owns |
| Bubbling `KeepAliveNotification` with a holder id and a liveness sweep | Foundation has zero production use; needs a four-site trait-signature change for an `&ElementTree`; index-keyed leases are relocation-unsafe; the sweep misses a `GlobalKey` retake, which reparents rather than removes |
| A `set_keep_alive(bool)` setter or a bool on a marker view | Last-writer-wins across N independent holders — the case Flutter uses a handle *set* for |
| An `InheritedView` `KeepAliveScope` (the `VsyncScope` idiom) | The host is a render behavior and cannot read inherited data; forces a wrapper element plus a config field on all three sliver widgets |
| A per-item aggregation element (porting `AutomaticKeepAlive`) | Unnecessary once the table is keyed by `ElementId`; would contend with the existing `RepaintBoundary` for the wrapper slot |
| `drop_child` park plus a keep-alive bucket | Parentless ⇒ standalone relayout root against stale constraints; solves a problem the placed-generation stamp already solved |
| Snapshot-and-restore into an LRU instead of parking | The only bounded-by-construction option, and the reason it is recorded here rather than dismissed. Rejected because it loses focus, in-flight animations and nested scroll offsets — most of what keep-alive is actually asked for |

## Oracle and replacement tests

`.flutter/packages/flutter/test/widgets/automatic_keep_alive_test.dart` (9 test
names, 12 executions — its body runs twice via `void tests({required bool
impliedMode})`).

| Flutter test | Disposition |
|---|---|
| `…with ListView with itemExtent` / `without itemExtent` / `with GridView` | Ported in spirit: a held item survives the band moving away, with an unheld control in the same list proving eviction ran |
| `AutomaticKeepAlive double` | Ported as the multi-holder law — releasing one holder keeps the child, releasing the last evicts it |
| `Keep alive Listenable has its listener removed once called` | **Retired**, replaced by construction: `Drop` *is* the release, pinned by a lease dropped with no explicit call |
| `keepAlive set to true before initState` | **Retired**: it pins Flutter's post-frame fallback for when the `KeepAlive` child element does not exist yet. FLUI's table has no such ordering — a lease taken in `init_state` is effective in that same frame |
| `AutomaticKeepAlive with SliverKeepAliveWidget` | **Dropped, recorded**: it exists to keep `RenderSliverWithKeepAliveMixin` usable by third-party slivers, an artifact of the parent-data design this ADR deletes |
| `AutomaticKeepAlive double 2` (reparenting a holder between slivers) | **Correct by construction, not by test.** A lease records only its *holder*; the sparse child is resolved from the tree when eviction asks, so a `GlobalKey` graft between two lists re-targets with no bookkeeping. There is no cached target that could go stale. Not pinned by a test: the unit fixture cannot host a real adaptor (`hosts_sparse_children` lives on the behavior, which only `SliverAdaptorBehavior` sets), and a two-list graft at the widget level is not yet written |
| `…and widget goes out of scope` (250 items, jump past the whole window) | **Named gap.** Not ported; would catch a guard that only works for incremental scrolls |

Deliberately out of scope, named rather than silently dropped: `_SelectionKeepAlive`
(no selection registrar is wired here), and the stale `layoutOffset` on a parked
child, which FLUI inherits identically.
