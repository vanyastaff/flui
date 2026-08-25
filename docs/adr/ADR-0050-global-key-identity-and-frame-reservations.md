# ADR-0050: GlobalKey identity, per-frame reservations, and the duplicate verdict

*A `GlobalKey` is identified by the key, never by its hash — every registry that answers "which element holds this key?" buckets on `ViewKey::key_hash` and decides with `ViewKey::key_eq`. Resolving a key at attach time stays optimistic (the graft is unchanged), but each declaration is now recorded against its declaring parent for the frame — as is each parent a graft robs without its consent — and the frame boundary verifies those records: one key claimed twice is repaired and reported as a typed `DuplicateGlobalKey`, not silently resolved by whoever asked last.*

---

- **Status:** Accepted (2026-08-24)
- **Date:** 2026-08-24
- **Deciders:** @vanyastaff
- **Scope:** the identity model of the intra-tree `GlobalKey` registry, the `GlobalKeyScope` claim table, and the ambient resolution handle behind `GlobalKey::current_element`; the two per-frame ledgers (declarations, and parents robbed by a graft), where each is recorded and where they are verified; the repair performed before a duplicate is reported; the channel a duplicate is reported through
- **Related:** [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md) (the two `GlobalKey` authorities — this ADR corrects its "key hash → …" wording to "key → …" and adds the per-frame ledgers alongside them); [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (owner-affine realms)
- **Issue:** #531 — verify GlobalKey reservations by identity at frame finalization

---

## Context

Two defects sat behind one symptom.

**A hash was being used as an identity.** `BuildOwner::global_keys` was a `HashMap<u64, ElementId>` keyed on `ViewKey::key_hash()`, and `GlobalKeyScope`'s claim table was keyed the same way. A hash is a lossy projection of a key; making it the identity means the framework cannot distinguish two genuinely different keys that happen to collide from one key used twice. That is not merely a lookup accident — the *retake* machinery reads that map, so a collision could hand one view's element to a completely unrelated key, and the duplicate-key check downstream of it could not tell which case it was looking at. The concrete population is safe today (`GlobalKey<T>` draws its id from one process-wide counter shared across every `T`, so live `GlobalKey`s never collide), but `is_global_key()` is an open trait method and the guarantee is not the registry's to assume.

**Nothing verified the optimism.** Attaching a keyed child resolves the key optimistically: if some element already holds it, that element is grafted here instead of a second one being mounted. Flutter does the same and says so in `_retakeInactiveElement`'s own comment — the "inactivity" is forward-looking, and "the only way that assumption could be false is if the global key is being duplicated". The graft is therefore not, on its own, evidence of anything: a legal reparent and an illegal duplicate look identical at the moment they happen. What separates them is the *frame*. Flutter records every declaration in `_debugGlobalKeyReservations` and checks it in `finalizeTree`; FLUI recorded nothing and checked nothing, so two parents declaring one key simply took turns grafting the element, leaving the loser silently empty and the developer with no signal at all.

The only shape FLUI did reject was the narrowest one — the same key twice under the *same* parent — and only in debug, because the check is a `debug_assert`-style panic. Every cross-parent case went unreported in every profile.

Flutter reaches its verdict through **three** cooperating mechanisms, not one: the reservation ledger above; `_debugElementsThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans`, which catches a graft out of a parent that never rebuilds; and `_debugVerifyIllFatedPopulation`, which watches the key registry for an element displaced by a second registration. Porting only the first leaves two real holes — both of which a review of the first draft of this change found, and both of which are closed below (§2b and §3).

## Decision

### 1. Identity is the key; the hash is an index

Three tables answer key questions, and all three now use the same rule: bucket on `key_hash()`, decide with `key_eq()`.

| Table | Question it answers | Where |
|---|---|---|
| `GlobalKeyRegistry` | which element in *this* owner's tree holds this key | `owner/global_key_registry.rs` |
| `GlobalKeyScope` | which *owner* currently holds this key | `owner/global_key_scope.rs` |
| `GlobalKeyReservations` | which parents declared this key *this frame*, and which parents a graft robbed | `owner/global_key_reservations.rs` |

Buckets are `Vec`s. Collisions are rare enough that a linear `key_eq` scan over one or two entries beats anything cleverer, and explicit enough that the identity check cannot be optimised away by accident. `Box<dyn ViewKey>` has no blanket `Hash + Eq` to hand a `HashMap` directly, which is why the two halves are written out rather than derived; Dart gets the same semantics for free because `GlobalKey` uses reference equality and `Map` keys on the object.

Consequently `register_global_key` / `unregister_global_key` / `element_for_global_key` / `take_global_key_for_reparent` take `&dyn ViewKey` rather than `u64`, on both `BuildOwner` and the `ElementOwner` split-borrow handle, and `ElementNode` stores the registered key itself (`registered_global_key`) instead of its hash — unregistering by hash alone could remove a colliding neighbour's entry. `registered_global_key_hash()` survives as a derived accessor for tracing and tests.

The ambient resolution handle behind `GlobalKey::current_element` / `with_current_state` takes the key too. A hash-only lookup at that boundary would be the one place a caller could still be handed a colliding stranger's element.

### 2. Declarations are reserved per frame

Every path where a parent declares a child carrying a `GlobalKey` records `parent -> (child -> key)`:

- a fresh mount (`ElementTree::insert`),
- a graft (`insert`'s retake branch),
- an in-place update of a keyed child the parent already had (`id_reconcile::update_child`).

The third is not optional. A parent that keeps its keyed child across a rebuild is still claiming the key this frame; without that record, a second parent grafting the element would look like a lone, legal reparent. Flutter records from `Element.updateChild`, which covers update and inflate alike.

A parent that gives a child up mid-frame withdraws its reservation (`id_reconcile::remove_child`), mirroring `_debugRemoveGlobalKeyReservationFor`. A root mount records nothing: a reservation names the declaring *parent*, and a root has none.

Reservations are held in declaration order — an ordered parent list plus each parent's own ordered declarations — so the duplicate report is reproducible run to run. Flutter iterates a `HashMap` here and its "older parent" is whichever the hash order surfaced first.

### 2b. Parents a graft robs are recorded too

Reservations alone cannot see the most ordinary cross-parent duplicate. When parent B declares a key that parent A is still holding, B's graft pulls the child out from under A — and if A never runs this frame, A never reserves, so the ledger has one claimant and reports nothing, while A ends the frame describing a child it no longer has.

So `retake_active_global_key` records the robbery against A, and **A rebuilding drops it**: rebuilding without the child is precisely how a parent consents to the loss. Anything still standing at the frame boundary is a parent that never consented. This is Flutter's third ledger, `_debugElementsThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans` (`framework.dart:3148`), recorded in `_retakeInactiveElement` and cleared by `_debugElementWasRebuilt`.

A rebuild clears a parent's *reservations* at the same point, for the same reason: a parent's newest build is the whole truth about what it declares, so an earlier build in the same frame that named a keyed child it has since dropped must not linger as a competing claim.

The two ledgers do not double-report: a robbery is not recorded when the robbed parent has already reserved that child itself, because the reservation walk will report that conflict.

### 3. Verification, repair, then report — at the frame boundary

`BuildOwner::finalize_tree` runs the inactive-element unmount sweep and then verifies both ledgers, matching where Flutter calls `_debugVerifyGlobalKeyReservation` (after `_inactiveElements._unmountAll`). Verification skips a parent no longer in the tree and a child that ends the frame with no parent — both cases describe a claim nobody kept — and skips a robbed parent whose child came home.

A key claimed twice is a duplicate. That includes one parent claiming it for two different children, which Flutter skips here (`framework.dart:3248`) and leaves to `_debugVerifyIllFatedPopulation`, its registry-watching third mechanism. FLUI has no third mechanism to leave it to, and the shape is reachable: the eager same-parent check in `retake_active_global_key` is debug-only, so in release the second attachment mounts a genuine second element under one key. Skipping it here would make "reports in every profile" false exactly where it matters.

Repair runs **before** the report is recorded: any parent still listing the contested child that is not its real parent has that edge dropped. A dangling child edge makes teardown cascade secondary failures on top of the real one, which is the same reason Flutter calls `forgetChild` there. Today the production graft already unlinks before it relinks, so the repair is usually a no-op — it is there so a report never leaves the tree worse than it found it.

Reservations are cleared by the same pass. A key legally moving from parent A in one frame to parent B in the next therefore sees a single claimant per frame and is never reported.

### 4. A duplicate is data, not a panic

A duplicate `GlobalKey` is caller-controlled input. It is surfaced as a typed `DuplicateGlobalKey` (naming the key, the contested child, and both parents in declaration order) through `BuildOwner::take_global_key_diagnostics`, plus a `tracing::error!`. The offending frame completes.

This is a deliberate divergence in *channel*, not verdict: Flutter throws a `FlutterError` out of `finalizeTree`. It also runs in every profile, where Flutter's entire reservation apparatus lives inside `assert(...)` and evaporates in release — a duplicate key corrupts a release tree exactly as badly as a debug one, and the cost is one small map per frame.

The two pre-existing eager panics are untouched, because neither is reachable from ordinary caller input in the same way: the same-parent duplicate (`retake_active_global_key`, debug-only) and the cross-owner scope conflict (`GlobalKeyScope`, fatal by ADR-0043's contract).

## Consequences

- ADR-0043 §2's "Split authority" paragraph reads "key hash to `ElementId`" / "key hash to which owner"; both are now "key to …", with the hash demoted to a bucket index. The split itself is unchanged, and gains a third, per-frame member that is neither of the other two: the ledgers are not an authority on where a key *is*, only on who *asked for it* this frame and who *lost it* this frame.
- `BuildOwner` grew by 32 bytes — the diagnostic drain (24) and one pointer to the ledgers (8) — and the size tripwire in `build_owner_tests.rs` moved from 512 to 576. The ledgers' own containers sit behind that pointer deliberately: they are frame scratch, empty in any tree that uses no `GlobalKey`s, and unboxing them would put the owner at 672.
- `ElementNode::child_ids()` became public. Observing a parent's own view of its children is the only way to see the dangling edge the repair clears — `parent()` answers the child's side, and the two disagreeing *is* the defect. The write side stays crate-internal to the reconciler.
- A host that wants Flutter's hard failure can drain the diagnostics and escalate. Nothing in the framework does so today.

## Alternatives considered

- **Keep hashing, document the collision risk.** This is what the code did, with an `§I4 hash-collision policy` comment saying so. It cannot be made correct by documentation: the retake path reads the map, so a collision is a state transplant, not a lookup miss. And it leaves the duplicate check unable to distinguish the two cases it exists to separate.
- **`HashMap<Box<dyn ViewKey>, ElementId>` with blanket `Hash + Eq` on the trait object.** Requires `ViewKey: Hash` and a manual `Eq` bridging `key_eq`, which drags a hashing contract onto every key implementor and makes `Box<dyn ViewKey>` silently order-sensitive in ways `key_eq` alone is not. Hash-bucket-plus-`key_eq` gets the same semantics with no new trait bounds.
- **Verify after `build_scope` instead of in `finalize_tree`.** Rejected for the reason Flutter places it after the unmount sweep: a key whose only remaining claimant is unmounted later in the same frame is not a duplicate, and verifying earlier would report it as one.
- **Panic on a duplicate, matching Flutter exactly.** Rejected per the issue's own direction: this is caller-controlled input, and a panic from the frame boundary leaves the tree non-resumable for a mistake the caller can fix. The typed diagnostic carries the same information and lets the host choose.
- **Port Flutter's third mechanism (`_debugVerifyIllFatedPopulation`) rather than folding the same-parent shape into the reservation walk.** That mechanism watches the *registry* for an element displaced by a second registration under one key. It would cover the same-parent release case and a little more (a key reused across two views of different types, where the retake declines and a second element is mounted) — but every reachable instance of "a little more" already produces two reservations under one key, so the reservation walk sees it. A third ledger with no population of its own is machinery, not coverage.
- **Debug-only recording, matching Flutter exactly.** Rejected: it makes the check absent precisely where a duplicate is hardest to diagnose, and the measured cost is one small map that is empty in every frame with no keyed children.
