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
//! `BuildOwner::global_keys` stays the intra-tree retake authority (key
//! → `ElementId` *in this tree*); `GlobalKeyScope` is the cross-tree
//! uniqueness authority (key → which owner currently holds it). Both index
//! by `ViewKey::key_hash` and decide by `ViewKey::key_eq`, so a hash is only
//! ever an accelerator and never an identity. The two
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

use flui_foundation::{ElementId, ViewKey};

use super::GlobalKeyRegistry;

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

/// One live claim: which owner currently holds a given `GlobalKey`.
///
/// The key is stored by value so the claim can be identity-compared later.
/// Its hash only picks the bucket — see [`ScopeState::claims`].
struct Claim {
    key: Box<dyn ViewKey>,
    owner: OwnerTag,
}

#[derive(Default)]
struct ScopeState {
    /// `key hash -> the claims sharing that hash`.
    ///
    /// Bucketed by hash, decided by [`ViewKey::key_eq`]: two distinct keys
    /// that collide are two claims in one bucket, never one claim standing
    /// in for both. A bucket is dropped as soon as it empties, so
    /// [`GlobalKeyScope::claim_count`] can sum bucket lengths without
    /// counting corpses.
    claims: HashMap<u64, Vec<Claim>>,
}

impl ScopeState {
    fn position_of(&self, key: &dyn ViewKey) -> Option<(u64, usize)> {
        let hash = key.key_hash();
        let index = self
            .claims
            .get(&hash)?
            .iter()
            .position(|claim| claim.key.key_eq(key))?;
        Some((hash, index))
    }
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
///    claim a key the first still holds.
/// 2. **A tag-checked no-op release** — a stale or duplicate release from an
///    owner that no longer holds the claim never disturbs whoever holds it
///    now (see this type's crate-internal `release` method).
/// 3. **Two live elements momentarily existing under one key, one in each
///    owner's own tree** — reachable only if a claim is force-reclaimed
///    (e.g. a direct crate-internal `reclaim_owner` call, not the
///    normal unmount path) while its original owner still has an inactive,
///    unfinalized retake candidate for that same key: a second owner can
///    then claim the key fresh, and the first owner's later intra-tree
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

    /// Number of `GlobalKey`s currently claimed in this scope.
    ///
    /// Diagnostic/test surface — production code never scans the scope by
    /// size, only by a single keyed lookup through the claim/release path.
    #[must_use]
    pub fn claim_count(&self) -> usize {
        self.state
            .borrow()
            .claims
            .values()
            .map(Vec::len)
            .sum::<usize>()
    }

    /// Attempt to claim `key` for `owner`.
    ///
    /// Returns an RAII [`ClaimGuard`] on success — the claim is live the
    /// moment this call returns `Ok`, but the guard releases it again on
    /// drop unless [`ClaimGuard::commit`] runs first, so a caller that
    /// claims and then unwinds (build-error panic) before finishing its own
    /// owner-map insert never leaks a claim nobody will ever release.
    /// Re-claiming a key this SAME owner already holds is not a conflict —
    /// it commits immediately, matching the existing last-write-wins
    /// re-registration behavior of a single owner's local map.
    ///
    /// Returns [`ClaimConflict`] when a DIFFERENT owner already holds
    /// `key`. The caller traces and panics — see
    /// `global_key_scope::claim_and_register`, the sole production call site.
    pub(crate) fn try_claim(
        &self,
        key: &dyn ViewKey,
        owner: OwnerTag,
    ) -> Result<ClaimGuard<'_>, ClaimConflict> {
        let mut state = self.state.borrow_mut();
        if let Some((hash, index)) = state.position_of(key) {
            let existing = &state.claims[&hash][index];
            if existing.owner != owner {
                return Err(ClaimConflict {
                    holder: existing.owner,
                });
            }
            // Same owner reclaiming its own key: nothing new was taken, so
            // the guard has nothing to roll back — pre-committed.
            drop(state);
            return Ok(ClaimGuard {
                scope: self,
                key: key.clone_key(),
                owner,
                committed: true,
            });
        }
        state.claims.entry(key.key_hash()).or_default().push(Claim {
            key: key.clone_key(),
            owner,
        });
        drop(state);
        Ok(ClaimGuard {
            scope: self,
            key: key.clone_key(),
            owner,
            committed: false,
        })
    }

    /// Tag-checked release: removes the claim on `key` only if `owner` is
    /// still its current holder.
    ///
    /// A release from an owner that no longer holds the claim (its own
    /// finalize ran late, after another owner already claimed the same
    /// key — the adversarial interleaving ADR-0043 names) is a
    /// traced no-op, never a mutation of whoever holds the claim now. No-op,
    /// untraced, if `key` has no live claim at all.
    pub(crate) fn release(&self, key: &dyn ViewKey, owner: OwnerTag) {
        let mut state = self.state.borrow_mut();
        let Some((hash, index)) = state.position_of(key) else {
            return;
        };
        let bucket = state
            .claims
            .get_mut(&hash)
            .expect("BUG: position_of resolved a bucket that is no longer present");
        if bucket[index].owner != owner {
            tracing::debug!(
                hash,
                releasing_owner = %owner,
                current_holder = %bucket[index].owner,
                "GlobalKeyScope::release: tag mismatch, ignoring — the claim was \
                 already reassigned to a different owner since this owner's release"
            );
            return;
        }
        bucket.swap_remove(index);
        if bucket.is_empty() {
            state.claims.remove(&hash);
        }
    }

    /// Extract every claim tagged to `owner` without retiring arbitrary keys.
    ///
    /// Called from [`BuildOwner`](super::BuildOwner)'s `Drop` impl so a
    /// dropped owner's stale claims cannot wedge the scope forever. Asserts
    /// nothing — an owner-drop reclaim is expected background cleanup, not a bug:
    /// an owner dropped with zero live claims (the common case — every key
    /// was already unregistered through the normal unmount path) reclaims
    /// silently. The caller retires keys after releasing its binding guard.
    pub(crate) fn take_owner_claims(&self, owner: OwnerTag) -> Vec<Box<dyn ViewKey>> {
        let mut state = self.state.borrow_mut();
        let mut removed = Vec::new();
        for bucket in state.claims.values_mut() {
            let mut index = 0;
            while index < bucket.len() {
                if bucket[index].owner == owner {
                    let claim = bucket.remove(index);
                    removed.push(claim.key);
                } else {
                    index += 1;
                }
            }
        }
        state.claims.retain(|_, bucket| !bucket.is_empty());
        removed
    }

    pub(crate) fn reclaim_owner(&self, owner: OwnerTag) -> usize {
        let removed = self.take_owner_claims(owner);
        let reclaimed = removed.len();
        let mut first = None;
        for key in removed {
            if first.is_some() || std::thread::panicking() {
                std::mem::forget(key);
            } else {
                crate::lifecycle::preserve(
                    &mut first,
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(key))).err(),
                );
            }
        }
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
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

/// A different owner already holds the key a [`GlobalKeyScope::try_claim`]
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
    key: Box<dyn ViewKey>,
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
            self.scope.release(self.key.as_ref(), self.owner);
        }
    }
}

/// Claim `key` for `owner` in `*scope` (lazily self-owning a private,
/// single-tenant scope when `*scope` is `None` — standalone/test owners
/// behave exactly as before this file existed, since a private scope with
/// one tenant never conflicts with itself), then record `key -> element`
/// in the owner's own local registry.
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
    key: &dyn ViewKey,
    element: ElementId,
    local: &mut GlobalKeyRegistry,
) {
    let scope_ref = scope.get_or_insert_with(GlobalKeyScope::new);
    match scope_ref.try_claim(key, owner) {
        Ok(guard) => {
            local.insert(key, element);
            guard.commit();
        }
        Err(conflict) => {
            let hash = key.key_hash();
            tracing::error!(
                hash,
                this_owner = %owner,
                holder = %conflict.holder,
                element = ?element,
                "GlobalKeyScope: cross-owner GlobalKey collision — a different owner \
                 already holds this key while sharing a GlobalKeyScope"
            );
            panic!(
                "GlobalKey {key:?} is already claimed by {} in this GlobalKeyScope; \
                 {} cannot mount {element:?} with the same key while both owners share \
                 the scope",
                conflict.holder, owner
            );
        }
    }
}

/// Release `key` from the owner's local registry and, tag-checked, from
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
    key: &dyn ViewKey,
    local: &mut GlobalKeyRegistry,
) {
    local.remove(key);
    if let Some(scope) = scope {
        scope.release(key, owner);
    }
}
