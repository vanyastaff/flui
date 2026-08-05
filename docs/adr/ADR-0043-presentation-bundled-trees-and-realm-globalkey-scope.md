# ADR-0043: Presentation-bundled UI trees and the realm GlobalKey scope

*Each presentation owns a complete, self-contained UI tree bundle — `WidgetsBinding`, `BuildOwner`, `ElementTree`, `PipelineCell`, and its own focus/IME/gesture/semantics state. A realm owns only what is genuinely cross-tree: scheduling, the post-frame lane, interaction dispatch, async driving, and `GlobalKey` uniqueness. `GlobalKeyScope` (`flui-view`) gives owners sharing a realm a cross-tree uniqueness domain without merging their trees — a duplicate `GlobalKey` across two presentations fails eagerly, and a key moving between presentations is always an unmount plus a fresh mount, never a live transplant.*

---

- **Status:** Accepted (2026-08-05)
- **Date:** 2026-08-05
- **Deciders:** @vanyastaff
- **Scope:** the per-presentation ownership topology for issue #555 (`PresentationState` bundle shape, realm-vs-presentation ownership split); `GlobalKeyScope` and its claim/release protocol in `flui-view` (implemented in this change); the frame-pump dirty predicate and the six-step presentation teardown contract (target design, implemented by the presentation-forest slice that follows); the divergences from Flutter's single-tree multi-view model this topology produces
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (owner-affine `UiRealm`s — the realm/thread-affinity model this ADR builds a presentation topology on top of, and whose §8 GlobalKey bullet this ADR corrects); [ADR-0037](ADR-0037-presentation-ownership-domains.md) (the three physical owners — `WindowHost`, `PresentationState`, `RasterOwner` — one presentation identity coordinates; this ADR is what `PresentationState` contains)
- **Issue:** #555 — realm-owned presentation forest and the production multi-window policy

---

## Context

ADR-0027 settled the realm/thread-affinity model — one `UiRealm` per independent unit of concurrency, `!Send` widget-tree types, a closed cross-thread command vocabulary — but left one question open: when a realm hosts more than one presentation (more than one window, or a headless embedder alongside a windowed one), what exactly does each presentation own, and what does the realm own on their behalf? ADR-0027 §8 answered part of this by assertion ("keys span all of that realm's presentations — Flutter's cross-view State preservation is recoverable by policy") without having designed the tree-ownership shape that claim depends on.

Two candidate shapes were evaluated before this one:

1. **One owner, N roots** — a single `BuildOwner` and `ElementTree` per realm, with each presentation mounting its own root into the shared tree. This was rejected: it makes `ElementId`/`RenderId` realm-unique again (reopening the exact per-tree slab-index simplicity ADR-0027's per-presentation `PipelineCell` work depended on), forces every tree-wide operation (`reassemble`, `finalize_tree`, the dirty heap) to reason about multiple disjoint root subtrees it was never built to distinguish, and ties one presentation's build storm to another's through one shared dirty heap and one shared inactive-elements queue — exactly the kind of coupling ADR-0027's realm isolation exists to avoid at the realm level, now reintroduced one level down.
2. **A hard `native window == runtime` split** — considered and rejected already in ADR-0027 (Alternative C) for the same reason it would still be wrong here: it cannot express a headless embedder or an offscreen presentation sharing a realm with a windowed one.

**Per-presentation bundles** — the shape this ADR settles — avoid both: each presentation is a fully independent tree with its own owner, so nothing about tree storage changes from what a single-presentation realm already does, and the realm's job is reduced to the services that are genuinely shared. This also matches the review discipline this design was held to before landing: two separate passes stress-tested the protocol below against concrete failure interleavings — a cross-owner key collision that must not corrupt the first owner's tree, an owner dropped without unmounting, and an owner racing its own retake against a second owner's fresh mount for the same key — before the shape was accepted; the red-exploit suite accompanying this change is that same set of scenarios turned into tests.

## Decision

### 1. Ownership topology

A presentation is a self-contained UI tree bundle: everything that builds, lays out, focuses, or presents for one surface lives in, and dies with, its `PresentationState`. Concretely, one `PresentationState` owns:

- `WidgetsBinding`, `BuildOwner`, `ElementTree`, `PipelineCell` (render tree + pipeline owner)
- focus, IME, gestures, semantics for that surface
- its own `needs_redraw` bit

A `UiRealm` owns only what is genuinely cross-tree:

- `Scheduler`, `LocalPostFrameLane`, `InteractionLane` (a dispatch handle installed into each owner)
- `AsyncDriver`
- the `GlobalKeyScope` (below)
- the frame pump that drives every presentation once per tick

Above realms, `SharedEngineServices` and application models are constructor-injected — the sanctioned way state crosses presentation and realm boundaries, never a shared tree.

**Hard rule:** `ElementId`/`RenderId` are per-tree slab indices, not realm-unique. Nothing at the realm level stores a bare tree id; a realm-level reference is always `PresentationId` plus a per-tree id. This is what keeps the one-owner-N-roots shape from creeping back in through a side door.

### 2. The `GlobalKeyScope` protocol

**Shape.** `GlobalKeyScope` lives in `flui-view` (`Rc<RefCell<…>>`, cheap to clone) as a realm-agnostic shared uniqueness domain — the type carries no notion of a realm or presentation, only "a set of owners agreeing to share one `GlobalKey` uniqueness domain." Wiring is a post-construction setter, `BuildOwner::set_global_key_scope`, called during presentation assembly before the owner's tree is mounted — the same pattern already established for `set_async_driver`/`set_post_frame_handle`/`set_text_input_handle`/`set_interaction_dispatch_handle`. An owner that never calls the setter self-owns a private scope lazily on its first `GlobalKey` registration, so standalone and test owners keep exactly today's single-tenant behavior with zero wiring.

**Split authority.** `BuildOwner::global_keys` stays the intra-tree retake authority — key hash to `ElementId` in this tree, exactly as it was. `GlobalKeyScope` is the cross-tree uniqueness authority — key hash to which owner currently holds it. The two never merge: `ElementTree`'s retake machinery (`try_retake_global_key`, `retake_inactive_global_key`) reads and writes only the local map and is completely unaware a scope exists. This is a deliberate, not incidental, property — see the "ElementTree untouched" note below.

**Claim lifetime.** A claim's lifetime is the owner-map lifetime: taken when `register_global_key` inserts into the local map (mount), released when `unregister_global_key` removes from it (unmount, via `finalize_tree`'s `remove_finalized`). Both hooks already existed; this change adds a scope claim/release inside them, piggybacked, with zero new hooks in `element_tree.rs` — the file is untouched, literally. An **inactive** element (soft-removed, pending finalize) keeps its claim, because nothing in the claim/release path runs at soft-remove time, only at register/unregister — which fire at mount and at finalize, never in between.

An earlier draft of this protocol specified that an inactive element's retained claim should not block a fresh mount of the same key in a different presentation. That guarantee is dropped here as unobservable: under the realm execution contract (single-thread serialized presentation segments, with an owner's own finalize running inside its own segment before any other owner's segment can observe the aftermath), the window in which a second owner could see a first owner's inactive-but-unfinalized claim does not exist in any real realm execution. What replaces it is a documented contract, not a behavioral guarantee for that specific case: see "Contract" below.

**Registration order and eager conflict.** Registering a key first attempts to claim it in the scope; a cross-owner duplicate — a different owner already holding the hash — fails eagerly, traced at error level and then panicked, naming both owners. This is the same verdict and timing as the pre-existing intra-tree duplicate-key panic in `element_tree.rs`'s `register_global_key_with_collision_check`, now widened to cross-owner scope rather than replacing it: the intra-tree check still runs first and is completely unchanged.

**Resolution stays realm-wide.** `GlobalKey::current_element`/`with_current_state` need to resolve a key to a live element without knowing which presentation holds it. That resolution layer — a realm-level composite registry handle assembled in `flui-app` over each presentation's `WidgetsBinding` registry, activated whole-frame by `UiRealm::enter()` exactly as today's single-registry activation is — is unchanged by this ADR and is not part of what lands with it; it is target design for the presentation-forest slice that follows. The composite is the *only* realm-facing resolution layer: per-presentation registries are its shards, `GlobalKeyScope` is its uniqueness index, one activation seam.

**Failure atomicity.** The claim is taken through an RAII guard: if the corresponding owner-map insert never happens — a panic mid-mount, a build error unwinding before the insert completes — the guard releases the claim on unwind rather than leaking it. Symmetrically, a dropped `BuildOwner` reclaims any claims still tagged to it (traced, not asserted): an owner that dropped without ever unmounting its tree is expected background cleanup, not a violation of anything.

### 3. Contract, not test choreography

The preconditions above are documented as a contract on `GlobalKeyScope` itself, not merely proven by the test suite: under single-thread serialized presentation segments with finalize discipline (an owner's deactivated keys finalize within its own segment), `GlobalKey` uniqueness across every owner sharing a scope is realm-scoped and eager — a key's state never silently migrates between owners, and a key is mountable in another owner exactly when its claim has been released by unmount or finalize. Violating these preconditions outside that contract — a hand-rolled multi-owner rig driving claim/release/retake/finalize in an order a real realm would never produce — still yields a *defined* outcome (an eager panic, or a tag-checked no-op release that never disturbs a claim it does not hold), never undefined behavior or a cross-tree state corruption. The adversarial-interleaving tests accompanying this change exercise exactly that boundary.

### 4. Frame pump dirty predicate (target design)

Per pump tick, presentations are processed once each, in mount order. At segment start for presentation P: clear P's wake bit, then sample `dirty(P) = P's owner heap non-empty OR P's external inbox non-empty` — the bit is wake-only, the heap/inbox sample is the truth, so marks arriving during P's own segment simply set the bit again for the next pump rather than being lost. Each presentation builds and flushes at most once per pump; intra-pump ping-pong is impossible by construction. This has a latency consequence, named rather than hidden: cross-presentation dirtying is mount-order-biased. A presentation dirtying a later-ordered sibling in the same pump lands together (zero extra frames); dirtying an earlier-ordered sibling costs one frame, because that sibling's segment already ran this tick. This lands with the presentation-forest slice, not with this change.

### 5. Presentation teardown (target design)

Tearing down one presentation is total for that bundle and structurally invisible to its siblings. The ordering contract is six steps, run at Idle outside the dispatch gate: (1) unregister from the window registry, stopping new routing; (2) detach IME, deactivate focus; (3) detach the root widget through that presentation's own binding — dispose callbacks run with capabilities installed and must route only within this presentation; (4) reclaim any `GlobalKeyScope` claims still tagged to the dead owner, traced, not asserted; (5) drop the `PresentationState` (pipeline, arena, semantics); a realm tears down when its last presentation closes. An async-task disposition step sits between (3) and (4): `AsyncDriver` is realm-level, so in-flight tasks spawned from the dead presentation's elements are not cancelled outright — their completions must fail closed against the dropped tree rather than reaching a live sibling by a raw id. This full ordering lands with the presentation-forest slice.

## Divergences from Flutter

Flutter's `WidgetsBinding` and `BuildOwner` are process-global singletons; a single `Element` tree spans every `View` (Flutter's own multi-window primitive). Per-presentation bundles are a deliberate divergence, inside the sanctioned leapfrog zone for multi-window ownership and runtime topology — Flutter is not the behavioral reference for this axis. Four consequences are worth naming so an application author does not discover them by surprise:

1. **Per-presentation sequential build → layout → paint, not Flutter's flush-everything-then-paint-everything.** Flutter's multi-view frame runs `flushLayout` for every view before `flushPaint` for any of them. FLUI runs build, layout, and paint for one presentation fully before moving to the next in mount order. No C1–C9 locked contract governs cross-window phase interleaving, so this is a topology choice, not a behavioral regression — but it means one presentation's paint can never observe another's layout from the *same* pump tick having already happened, where Flutter's phased flush would guarantee it had.
2. **Cross-presentation dirtying has a mount-order latency bias** (§4 above): the presentation-forest frame pump processes presentations once each in mount order, so a later-ordered sibling dirtied mid-pump joins the same pump for free, while an earlier-ordered sibling dirtied mid-pump waits one frame. Flutter's single-tree model has no equivalent asymmetry because there is only one tree to flush.
3. **Cross-window `InheritedWidget` scope is inexpressible.** Flutter's single element tree lets an inherited scope (a theme, a localization, an app-wide setting) span every `View` for free, because they are all elements in the same tree. FLUI's N independent trees mean the same app-wide state must be re-wrapped at the root of every presentation that needs it — this is the divergence an application author is most likely to actually hit. The sanctioned shape for state that must be visible everywhere is a shared model injected above the realm (`SharedEngineServices` or an app-level model), republished through each presentation's own inherited widget at its root, not a cross-tree inherited lookup.
4. **`WindowPolicy` is not behavior-neutral.** A future embedder-level policy choosing between N realms (one per window, the default) and one realm hosting N presentations flips the duplicate-`GlobalKey` verdict: under N realms, the same key mounted in two windows is two independent `GlobalKeyScope`s and is allowed; under one realm hosting N presentations, it is the eager cross-owner panic this ADR specifies. This is not a bug in either policy — it is what "uniqueness is scoped to what shares a `GlobalKeyScope`" necessarily means — but it means the policy an embedder picks changes what counts as a duplicate key, and that needs to be in that policy's own documentation, not discovered at a panic site.

## What is untouched

`ElementTree` and `WidgetsBinding` are untouched by the `flui-view` half of this change — literally: `crates/flui-view/src/tree/element_tree.rs` has zero lines changed. `retake_inactive_global_key` keeps its existing intra-tree semantics exactly as they were; it does not consult `GlobalKeyScope` and was never intended to, because the split-authority design in §2 makes that correct rather than an oversight — retake is a same-owner event, and the scope claim for a same-owner event never changes hands regardless of whether the reactivating call site happens to check it. Zero new `BuildContext` capability tokens are introduced; `scripts/check-frame-capability-scope.sh` is unaffected. `BuildOwner::focus_manager()`/`text_input_handle()` are unchanged.

`BuildOwner::set_global_key_scope` is classified an additive, minor public API change — `BuildOwner` and `WidgetsBinding` are public surface (re-exported through `flui::app::*`, consumed by `flui-hot-reload`), and this is a new method with no change to any existing signature.

## Alternatives rejected

- **One owner, N roots.** Covered in Context — reintroduces realm-unique `ElementId`, couples every presentation's dirty heap and inactive-elements queue to its siblings, and forces `reassemble`/`finalize_tree` to reason about disjoint root subtrees they were never built to distinguish.
- **A single process-global `GlobalKey` registry (Flutter's literal shape).** Rejected by ADR-0027 already; repeated here only to note that per-presentation bundles do not reopen that question — the registry stays realm-scoped, and `GlobalKeyScope` is the mechanism that makes a *realm's* registry span multiple *presentations* without merging their trees.
- **Cross-presentation live State transplant on GlobalKey reparent.** Considered and rejected: it requires either a shared tree (the one-owner-N-roots shape, rejected above) or a cross-tree move primitive that copies `State` between two independent `ElementTree`s bypassing their owners' lifecycles — neither is a reparent in any sense the existing retake machinery implements, and inventing one now forecloses a better-designed continuity primitive later. A key moving between presentations is an unmount plus a fresh mount; a future cross-presentation continuity story is a dedicated checkpoint/restore primitive, designed on its own merits, not a decorated reparent.

## Follow-up

The frame-pump dirty predicate (§4) and the six-step presentation teardown contract (§5) are target design recorded here so the protocol they depend on — `GlobalKeyScope`'s claim/release/reclaim mechanism — is designed once, correctly, rather than re-derived per slice. They are implemented by the presentation-forest slice of issue #555, tracked separately, along with the realm-level composite `GlobalKey` registry handle and the routing/`FocusCoordinator` work that follows it.
