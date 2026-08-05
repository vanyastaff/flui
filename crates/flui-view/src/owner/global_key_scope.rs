//! `GlobalKeyScope` — a shared cross-owner `GlobalKey` uniqueness domain
//! (ADR-0043).
//!
//! # Why this exists
//!
//! Per-presentation topology (ADR-0043) gives every presentation its own
//! [`BuildOwner`](super::BuildOwner) and `ElementTree`. Each owner's
//! `global_keys` map (key hash → `ElementId`) is therefore intra-tree
//! only — sufficient for the reparent/retake machinery that already lives
//! in `crate::tree::element_tree`, but blind to a second owner mounting the
//! same `GlobalKey` in a different tree. `GlobalKeyScope` is the shared,
//! realm-agnostic index that closes that gap: every owner that shares one
//! scope participates in one cross-owner uniqueness domain, so a duplicate
//! `GlobalKey` mount fails eagerly instead of silently aliasing.
//!
//! `flui-view` knows nothing about realms or presentations — a scope is
//! just "a set of owners agreeing to share one uniqueness domain". Wiring
//! one scope per realm is a decision made above this crate, at presentation
//! assembly time, via [`BuildOwner::set_global_key_scope`](super::BuildOwner::set_global_key_scope).
//!
//! # Split authority
//!
//! `BuildOwner::global_keys` stays the intra-tree retake authority (key hash
//! → `ElementId` *in this tree*); `GlobalKeyScope` is the cross-tree
//! uniqueness authority (key hash → which owner currently holds it). The two
//! never merge: `ElementTree`'s retake machinery
//! (`try_retake_global_key`/`retake_inactive_global_key`) reads and writes
//! only the local map and is completely unaware a scope exists — this file,
//! and the two call sites in [`BuildOwner`](super::BuildOwner) /
//! [`ElementOwner`](super::ElementOwner), are the only places a scope claim
//! is taken or released.
//!
//! # Claim lifetime
//!
//! A claim's lifetime is the owner-map lifetime: taken when
//! `register_global_key` inserts into the local map (mount), released when
//! `unregister_global_key` removes from it (unmount/finalize) — tag-checked,
//! so a stale release from an owner that no longer holds the claim is a
//! traced no-op rather than a corruption of whoever holds it now (see
//! [`GlobalKeyScope::release`]). An **inactive** element (soft-removed,
//! pending finalize) keeps its claim: nothing in this file runs at
//! soft-remove time, only at the `register_global_key`/`unregister_global_key`
//! call sites the finalize/mount paths already drive.
//!
//! # Contract
//!
//! The execution contract this scope is designed for, the three defined
//! outcomes of violating it, and why the third one cannot arise inside a
//! real realm are documented on [`GlobalKeyScope`] itself — the type this
//! module exists to define — not here.

use std::{
    cell::RefCell,
    collections::HashMap,
    fmt,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use flui_foundation::ElementId;

/// Identifies one [`BuildOwner`](super::BuildOwner) for `GlobalKeyScope`
/// claim tagging.
///
/// Assigned once, from a process-wide monotonic counter, at owner
/// construction — stable for the owner's whole lifetime whether or not a
/// scope is ever installed. Never exposed on `BuildOwner`'s public surface:
/// callers observe conflicts and reclamation through tracing and panics, not
/// by comparing tags directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OwnerTag(u64);

impl OwnerTag {
    /// Mint a fresh, process-unique tag.
    pub(crate) fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for OwnerTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "owner#{}", self.0)
    }
}

/// One live claim: which owner currently holds a `GlobalKey` hash.
struct Claim {
    owner: OwnerTag,
}

#[derive(Default)]
struct ScopeState {
    claims: HashMap<u64, Claim>,
}

/// A realm-agnostic shared uniqueness domain for `GlobalKey`s across
/// multiple [`BuildOwner`]s.
///
/// Cheap to clone (`Rc`-backed): every owner sharing one realm's scope holds
/// its own clone of the same handle. `flui-view` is presentation-agnostic —
/// this type carries no notion of a realm or a presentation, only "a set of
/// owners agreeing to share one `GlobalKey` uniqueness domain". Construct
/// one per realm at presentation-assembly time and install it into each
/// owner via [`BuildOwner::set_global_key_scope`](super::BuildOwner::set_global_key_scope).
///
/// # Contract
///
/// This scope is designed for one execution contract: presentation segments
/// run serialized on a single thread, and an owner's own finalize (its
/// deactivated keys being unmounted for real) runs inside its own segment —
/// never observed mid-flight by a different owner's segment. Under that
/// contract, `GlobalKey` uniqueness across every owner sharing a scope is
/// realm-scoped and eager: a key's state never silently migrates between
/// owners, and a key is mountable in another owner exactly when its claim
/// has been released by unmount or finalize.
///
/// This type does not enforce that contract — it cannot see thread
/// scheduling or segment boundaries, only claims. Violating the contract
/// (a hand-rolled multi-owner rig driving claim/release/retake/finalize in
/// an order a real realm would never produce) still yields one of three
/// defined outcomes, never undefined behavior:
///
/// 1. **An eager panic naming both owners** — the ordinary case: a
///    cross-owner conflict is detected the moment a second owner tries to
///    claim a hash the first still holds.
/// 2. **A tag-checked no-op release** — a stale or duplicate release from an
///    owner that no longer holds the claim never disturbs whoever holds it
///    now (see this type's crate-internal `release` method).
/// 3. **Two live elements momentarily existing under one key, one in each
///    owner's own tree** — reachable only if a claim is force-reclaimed
///    (e.g. a direct crate-internal `reclaim_owner` call, not the
///    normal unmount path) while its original owner still has an inactive,
///    unfinalized retake candidate for that same key: a second owner can
///    then claim the hash fresh, and the first owner's later intra-tree
///    retake — which never consults this scope, by design (see the module
///    docs' "Split authority" section) — still succeeds on its own terms,
///    reactivating its own candidate. This is confined, not a corruption in
///    the memory-unsafety sense: each owner's element is legitimate inside
///    its own tree, and the duplicate self-heals the moment either one is
///    genuinely unmounted, because that release is tag-checked against
///    whoever currently holds the claim (outcome 2). It cannot arise inside
///    a real realm: every realm-native path that frees an owner's claim
///    either runs that owner's own finalize first (which destroys the
///    retake candidate `remove_finalized` would otherwise leave behind) or
///    drops the owner outright (destroying every candidate it could ever
///    retake) — there is no realm path that reclaims a live claim while its
///    owner still has an unfinalized candidate sitting on the other side of
///    it. Reaching outcome 3 requires calling `reclaim_owner` directly,
///    which no realm-native code path does.
///
/// Outcome 1's panic is a fatal application bug, not a recoverable error: the
/// panicking owner's tree is left in a non-resumable state (the rejected
/// element's mount never completed), matching the long-standing semantics of
/// the pre-existing intra-tree duplicate-`GlobalKey` panic this one is
/// widened from. A host that catches it and keeps using that owner's tree is
/// operating on a torn mount, not a recovered one.
///
/// [`BuildOwner`]: super::BuildOwner
#[derive(Clone)]
pub struct GlobalKeyScope {
    state: Rc<RefCell<ScopeState>>,
}

impl GlobalKeyScope {
    /// Create a fresh, empty scope with no live claims.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(ScopeState::default())),
        }
    }

    /// Number of `GlobalKey` hashes currently claimed in this scope.
    ///
    /// Diagnostic/test surface — production code never scans the scope by
    /// size, only by a single hash lookup through the claim/release path.
    #[must_use]
    pub fn claim_count(&self) -> usize {
        self.state.borrow().claims.len()
    }

    /// Attempt to claim `hash` for `owner`.
    ///
    /// Returns an RAII [`ClaimGuard`] on success — the claim is live the
    /// moment this call returns `Ok`, but the guard releases it again on
    /// drop unless [`ClaimGuard::commit`] runs first, so a caller that
    /// claims and then unwinds (build-error panic) before finishing its own
    /// owner-map insert never leaks a claim nobody will ever release.
    /// Re-claiming a hash this SAME owner already holds is not a conflict —
    /// it commits immediately, matching the existing last-write-wins
    /// re-registration behavior of a single owner's local map.
    ///
    /// Returns [`ClaimConflict`] when a DIFFERENT owner already holds
    /// `hash`. The caller traces and panics — see
    /// `global_key_scope::claim_and_register`, the sole production call site.
    pub(crate) fn try_claim(
        &self,
        hash: u64,
        owner: OwnerTag,
    ) -> Result<ClaimGuard<'_>, ClaimConflict> {
        let mut state = self.state.borrow_mut();
        if let Some(existing) = state.claims.get(&hash) {
            if existing.owner != owner {
                return Err(ClaimConflict {
                    holder: existing.owner,
                });
            }
            // Same owner reclaiming its own hash: nothing new was taken, so
            // the guard has nothing to roll back — pre-committed.
            drop(state);
            return Ok(ClaimGuard {
                scope: self,
                hash,
                owner,
                committed: true,
            });
        }
        state.claims.insert(hash, Claim { owner });
        drop(state);
        Ok(ClaimGuard {
            scope: self,
            hash,
            owner,
            committed: false,
        })
    }

    /// Tag-checked release: removes the claim on `hash` only if `owner` is
    /// still its current holder.
    ///
    /// A release from an owner that no longer holds the claim (its own
    /// finalize ran late, after another owner already claimed the same
    /// hash — the adversarial interleaving ADR-0043 names) is a
    /// traced no-op, never a mutation of whoever holds the claim now. No-op,
    /// untraced, if `hash` has no live claim at all.
    pub(crate) fn release(&self, hash: u64, owner: OwnerTag) {
        let mut state = self.state.borrow_mut();
        match state.claims.get(&hash) {
            Some(claim) if claim.owner == owner => {
                state.claims.remove(&hash);
            }
            Some(claim) => {
                tracing::debug!(
                    hash,
                    releasing_owner = %owner,
                    current_holder = %claim.owner,
                    "GlobalKeyScope::release: tag mismatch, ignoring — the claim was \
                     already reassigned to a different owner since this owner's release"
                );
            }
            None => {}
        }
    }

    /// Traced reclamation of every claim still tagged to `owner`.
    ///
    /// Called from [`BuildOwner`](super::BuildOwner)'s `Drop` impl so a
    /// dropped owner's stale claims cannot wedge the scope forever. Asserts
    /// nothing — an owner-drop reclaim is expected background cleanup, not a bug:
    /// an owner dropped with zero live claims (the common case — every key
    /// was already unregistered through the normal unmount path) reclaims
    /// silently. Returns the number of claims reclaimed.
    pub(crate) fn reclaim_owner(&self, owner: OwnerTag) -> usize {
        let mut state = self.state.borrow_mut();
        let before = state.claims.len();
        state.claims.retain(|_, claim| claim.owner != owner);
        let reclaimed = before - state.claims.len();
        if reclaimed > 0 {
            tracing::debug!(
                owner = %owner,
                reclaimed,
                "GlobalKeyScope: reclaimed stale claims tagged to a dropped owner"
            );
        }
        reclaimed
    }
}

impl Default for GlobalKeyScope {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for GlobalKeyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalKeyScope")
            .field("claims", &self.claim_count())
            .finish()
    }
}

/// A different owner already holds the hash a [`GlobalKeyScope::try_claim`]
/// call was attempting.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClaimConflict {
    pub(crate) holder: OwnerTag,
}

/// RAII handle over one successful [`GlobalKeyScope::try_claim`].
///
/// Releases the claim on drop unless [`Self::commit`] ran first. The sole
/// production caller ([`claim_and_register`]) commits immediately after its
/// own local-map insert — an infallible `HashMap::insert` — but the guard
/// exists so that invariant is enforced by construction rather than by
/// caller discipline: anything landing between a future claim and its
/// commit that unwinds (a build error, a panic) still leaves the scope
/// claim-free, never a leaked claim nobody can ever release.
#[derive(Debug)]
pub(crate) struct ClaimGuard<'a> {
    scope: &'a GlobalKeyScope,
    hash: u64,
    owner: OwnerTag,
    committed: bool,
}

impl ClaimGuard<'_> {
    /// Confirm the claim — the owner-map insert this guard was protecting
    /// completed. After this call, dropping the guard does nothing.
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.scope.release(self.hash, self.owner);
        }
    }
}

/// Claim `hash` for `owner` in `*scope` (lazily self-owning a private,
/// single-tenant scope when `*scope` is `None` — standalone/test owners
/// behave exactly as before this file existed, since a private scope with
/// one tenant never conflicts with itself), then record `hash -> element`
/// in the owner's own local map.
///
/// On a cross-owner conflict: traces at error level, then panics naming
/// both owners — same verdict and timing as the existing intra-tree
/// duplicate-`GlobalKey` panic in `crate::tree::element_tree`
/// (`register_global_key_with_collision_check`), now widened to cross-owner
/// scope. Panicking here means the local `insert` below never runs, so the
/// caller's own bookkeeping and this scope agree: neither records the
/// rejected mount.
///
/// The sole two call sites are [`BuildOwner::register_global_key`](super::BuildOwner::register_global_key)
/// and [`ElementOwner::register_global_key`](super::ElementOwner::register_global_key) —
/// kept in one place so the claim-then-local-insert sequence, and its
/// panic/commit ordering, cannot drift between the two.
pub(crate) fn claim_and_register(
    scope: &mut Option<GlobalKeyScope>,
    owner: OwnerTag,
    hash: u64,
    element: ElementId,
    local: &mut HashMap<u64, ElementId>,
) {
    let scope_ref = scope.get_or_insert_with(GlobalKeyScope::new);
    match scope_ref.try_claim(hash, owner) {
        Ok(guard) => {
            local.insert(hash, element);
            guard.commit();
        }
        Err(conflict) => {
            tracing::error!(
                hash,
                this_owner = %owner,
                holder = %conflict.holder,
                element = ?element,
                "GlobalKeyScope: cross-owner GlobalKey collision — a different owner \
                 already holds this key while sharing a GlobalKeyScope"
            );
            panic!(
                "GlobalKey hash {hash} is already claimed by {} in this GlobalKeyScope; \
                 {} cannot mount {element:?} with the same key while both owners share \
                 the scope",
                conflict.holder, owner
            );
        }
    }
}

/// Release `hash` from the owner's local map and, tag-checked, from
/// `scope` (when installed).
///
/// The sole two call sites are [`BuildOwner::unregister_global_key`](super::BuildOwner::unregister_global_key)
/// and [`ElementOwner::unregister_global_key`](super::ElementOwner::unregister_global_key).
/// A `None` scope means `claim_and_register` was never called for this
/// owner (nothing was ever claimed), so there is nothing to release beyond
/// the local map.
pub(crate) fn release_and_unregister(
    scope: Option<&GlobalKeyScope>,
    owner: OwnerTag,
    hash: u64,
    local: &mut HashMap<u64, ElementId>,
) {
    local.remove(&hash);
    if let Some(scope) = scope {
        scope.release(hash, owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eid(n: usize) -> ElementId {
        ElementId::new(n)
    }

    /// Extract a panic payload's message, whether the panic macro produced a
    /// `&'static str` (the `panic!("literal")` fast path) or a `String` (the
    /// `panic!("{a} {b}")` interpolated path this module's panics use).
    fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
        if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            String::from("<non-string panic payload>")
        }
    }

    #[test]
    fn fresh_scope_has_no_claims() {
        let scope = GlobalKeyScope::new();
        assert_eq!(scope.claim_count(), 0);
    }

    #[test]
    fn claim_then_commit_is_visible_and_reclaimable() {
        let scope = GlobalKeyScope::new();
        let a = OwnerTag::fresh();

        let guard = scope.try_claim(1, a).expect("fresh hash claims");
        guard.commit();
        assert_eq!(scope.claim_count(), 1);

        assert_eq!(scope.reclaim_owner(a), 1);
        assert_eq!(scope.claim_count(), 0);
    }

    #[test]
    fn cross_owner_claim_on_a_live_hash_conflicts() {
        let scope = GlobalKeyScope::new();
        let a = OwnerTag::fresh();
        let b = OwnerTag::fresh();

        scope.try_claim(1, a).expect("fresh hash claims").commit();

        let conflict = scope
            .try_claim(1, b)
            .expect_err("b must not claim a's live hash");
        assert_eq!(conflict.holder, a);
        // The failed attempt must not have mutated the claim.
        assert_eq!(scope.claim_count(), 1);
    }

    #[test]
    fn same_owner_reclaiming_its_own_hash_is_not_a_conflict() {
        let scope = GlobalKeyScope::new();
        let a = OwnerTag::fresh();

        scope.try_claim(1, a).expect("first claim").commit();
        let guard = scope
            .try_claim(1, a)
            .expect("same owner re-claiming its own hash is not a conflict");
        guard.commit();
        assert_eq!(scope.claim_count(), 1);
    }

    /// An unwind between a successful claim and the caller's own
    /// commit (the local owner-map insert, in production always the very
    /// next infallible statement) must not leak the claim. This exercises
    /// the `ClaimGuard` contract directly and in isolation — the caller's
    /// commit never runs — proving the guard, not the surrounding
    /// production call sequence, is what closes the window.
    #[test]
    fn panic_mid_mount_releases_claim_via_guard() {
        let scope = GlobalKeyScope::new();
        let a = OwnerTag::fresh();
        let b = OwnerTag::fresh();

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = scope.try_claim(1, a).expect("fresh hash claims");
            // Simulate a build-error unwind landing before this owner's own
            // `commit()` (the local-map insert) ever runs.
            panic!("simulated mid-mount build error");
        }));
        assert!(outcome.is_err());

        // The guard's `Drop` ran during unwind and released the claim —
        // nothing was ever committed, so a different owner can claim the
        // same hash cleanly afterward.
        assert_eq!(scope.claim_count(), 0);
        scope
            .try_claim(1, b)
            .expect("the aborted claim must not have leaked")
            .commit();
        assert_eq!(scope.claim_count(), 1);
    }

    #[test]
    fn stale_release_from_an_owner_that_no_longer_holds_the_claim_is_a_no_op() {
        let scope = GlobalKeyScope::new();
        let a = OwnerTag::fresh();
        let b = OwnerTag::fresh();

        scope.try_claim(1, a).expect("a claims first").commit();
        scope.release(1, a); // a releases normally.
        scope.try_claim(1, b).expect("b claims fresh").commit();

        // A's late/duplicate release (e.g. a stray second unregister call)
        // must not touch b's now-live claim.
        scope.release(1, a);
        assert_eq!(
            scope.claim_count(),
            1,
            "b's claim must survive a's stale release"
        );

        let conflict = scope
            .try_claim(1, OwnerTag::fresh())
            .expect_err("the surviving claim must still be b's, not free");
        assert_eq!(conflict.holder, b);
    }

    #[test]
    fn reclaim_owner_only_touches_that_owners_claims() {
        let scope = GlobalKeyScope::new();
        let a = OwnerTag::fresh();
        let b = OwnerTag::fresh();

        scope.try_claim(1, a).expect("a claims 1").commit();
        scope.try_claim(2, b).expect("b claims 2").commit();

        assert_eq!(scope.reclaim_owner(a), 1);
        assert_eq!(scope.claim_count(), 1);
        // b's claim is untouched.
        let conflict = scope
            .try_claim(2, OwnerTag::fresh())
            .expect_err("b's claim must survive reclaiming a");
        assert_eq!(conflict.holder, b);
    }

    #[test]
    fn claim_and_register_records_local_map_on_success() {
        let mut scope = None;
        let owner = OwnerTag::fresh();
        let mut local = HashMap::new();

        claim_and_register(&mut scope, owner, 7, eid(1), &mut local);
        assert_eq!(local.get(&7), Some(&eid(1)));
        assert_eq!(scope.as_ref().map(GlobalKeyScope::claim_count), Some(1));
    }

    #[test]
    fn claim_and_register_panics_on_cross_owner_conflict() {
        let shared = GlobalKeyScope::new();
        let owner_a = OwnerTag::fresh();
        let owner_b = OwnerTag::fresh();
        let mut local_a = HashMap::new();
        let mut local_b = HashMap::new();
        let mut scope_a = Some(shared.clone());
        let mut scope_b = Some(shared);

        claim_and_register(&mut scope_a, owner_a, 3, eid(1), &mut local_a);

        // owner_b sharing the same scope must not silently alias owner_a's
        // claim; local_b stays untouched because the panic unwinds before
        // the insert.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            claim_and_register(&mut scope_b, owner_b, 3, eid(2), &mut local_b);
        }));
        let payload = outcome.expect_err("owner_b must not silently alias owner_a's live claim");
        let message = panic_message(&*payload);
        assert!(
            message.contains(&owner_a.to_string()),
            "panic must name the claim's current holder (owner_a): {message}"
        );
        assert!(
            message.contains(&owner_b.to_string()),
            "panic must name the rejected owner (owner_b): {message}"
        );
        assert_eq!(
            local_b.get(&3),
            None,
            "local_b stays untouched because the panic unwinds before the insert"
        );
    }

    #[test]
    fn release_and_unregister_clears_local_map_and_scope() {
        let mut scope = None;
        let owner = OwnerTag::fresh();
        let mut local = HashMap::new();

        claim_and_register(&mut scope, owner, 9, eid(1), &mut local);
        release_and_unregister(scope.as_ref(), owner, 9, &mut local);

        assert_eq!(local.get(&9), None);
        assert_eq!(scope.as_ref().map(GlobalKeyScope::claim_count), Some(0));
    }
}
