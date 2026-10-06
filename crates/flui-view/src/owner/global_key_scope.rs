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
//! [`GlobalKeyScope::take_claim`]). An **inactive** element (soft-removed,
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

use super::{GlobalKeyRegistry, global_key_registry::OwnedGlobalKey};

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
        Self::fresh_with_counter(&NEXT)
    }

    // Zero permanently records exhaustion after the last nonzero identity.
    // The local counter seam lets boundary tests avoid mutating process state.
    pub(super) fn fresh_with_counter(counter: &AtomicU64) -> Self {
        let tag = counter
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current == 0 {
                    None
                } else {
                    Some(current.checked_add(1).unwrap_or(0))
                }
            })
            .expect("OwnerTag counter exhausted: all nonzero u64 identities issued");
        Self(tag)
    }
}

impl fmt::Display for OwnerTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "owner#{}", self.0)
    }
}

/// One live claim: which owner currently holds a given `GlobalKey`.
///
/// The owned key can be pinned across a comparison without calling user cloning.
/// Its hash only picks the bucket — see [`ScopeState::claims`].
struct Claim {
    key: ScopedKeyOwner,
    owner: OwnerTag,
    marker: Rc<()>,
}

/// Passive admission identity: copying it neither invokes a key nor releases it.
#[derive(Clone, Debug)]
pub(super) struct ClaimIdentity {
    hash: u64,
    owner: OwnerTag,
    marker: Rc<()>,
}

/// One scope or comparison owner of one independently guarded key envelope.
pub(crate) struct ScopedKeyOwner(Option<Rc<OwnedGlobalKey>>);

impl ScopedKeyOwner {
    fn new(key: Box<dyn ViewKey>) -> Self {
        Self(Some(Rc::new(OwnedGlobalKey::new(key))))
    }

    fn as_ref(&self) -> &dyn ViewKey {
        self.0
            .as_ref()
            .expect("BUG: live scoped key is occupied")
            .as_ref()
            .as_ref()
    }

    fn retain(mut self) {
        std::mem::forget(self.0.take());
    }
}

impl Clone for ScopedKeyOwner {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Drop for ScopedKeyOwner {
    fn drop(&mut self) {
        let key = self.0.take();
        if std::thread::panicking() {
            std::mem::forget(key);
        } else {
            drop(key);
        }
    }
}

struct SnapshotClaim {
    key: ScopedKeyOwner,
    identity: ClaimIdentity,
}

/// Refuse a key whose comparison mutated the scope bucket on every attempt.
fn refuse_unstable_comparison() -> ! {
    panic!("GlobalKey comparison changed scope repeatedly")
}

fn retire_snapshot(snapshot: Vec<SnapshotClaim>) {
    let mut first = None;
    for claim in snapshot {
        if first.is_some() || std::thread::panicking() {
            claim.key.retain();
        } else {
            crate::lifecycle::preserve(
                &mut first,
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(claim.key))).err(),
            );
        }
    }
    if let Some(payload) = first {
        std::panic::resume_unwind(payload);
    }
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
    fn matches_snapshot(&self, hash: u64, snapshot: &[SnapshotClaim]) -> bool {
        let Some(bucket) = self.claims.get(&hash) else {
            return snapshot.is_empty();
        };
        bucket.len() == snapshot.len()
            && bucket.iter().zip(snapshot).all(|(claim, saved)| {
                claim.owner == saved.identity.owner
                    && Rc::ptr_eq(&claim.marker, &saved.identity.marker)
            })
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

    fn snapshot(&self, hash: u64) -> Vec<SnapshotClaim> {
        let state = self.state.borrow();
        state.claims.get(&hash).map_or_else(Vec::new, |bucket| {
            bucket
                .iter()
                .map(|claim| SnapshotClaim {
                    key: claim.key.clone(),
                    identity: ClaimIdentity {
                        hash,
                        owner: claim.owner,
                        marker: Rc::clone(&claim.marker),
                    },
                })
                .collect()
        })
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
        let hash = key.key_hash();
        let mut prepared = None;
        // One retry: a second mismatch means the key's comparison mutates
        // the bucket every time it runs, and that is refused.
        let mut retried = false;
        loop {
            let snapshot = self.snapshot(hash);
            let matching = snapshot.iter().find(|claim| claim.key.as_ref().key_eq(key));
            let identity = matching.map(|claim| claim.identity.clone());
            if identity.is_none() && prepared.is_none() {
                prepared = Some(ScopedKeyOwner::new(key.clone_key()));
            }
            let mut state = self.state.borrow_mut();
            if !state.matches_snapshot(hash, &snapshot) {
                drop(state);
                if retried {
                    refuse_unstable_comparison();
                }
                retried = true;
                retire_snapshot(snapshot);
                continue;
            }
            if let Some(identity) = identity {
                drop(state);
                if identity.owner != owner {
                    retire_snapshot(snapshot);
                    drop(prepared);
                    return Err(ClaimConflict {
                        holder: identity.owner,
                    });
                }
                let guard = ClaimGuard {
                    scope: self,
                    owner,
                    claim: None,
                    identity,
                };
                retire_snapshot(snapshot);
                drop(prepared);
                return Ok(guard);
            }
            let identity = ClaimIdentity {
                hash,
                owner,
                marker: Rc::new(()),
            };
            state.claims.entry(hash).or_default().push(Claim {
                key: prepared
                    .take()
                    .expect("BUG: absent claim has prepared ownership"),
                owner,
                marker: Rc::clone(&identity.marker),
            });
            drop(state);
            // Rollback exists before retiring any snapshot or losing owner.
            let guard = ClaimGuard {
                scope: self,
                owner,
                claim: Some((hash, Rc::clone(&identity.marker))),
                identity,
            };
            retire_snapshot(snapshot);
            return Ok(guard);
        }
    }

    /// Resolve a missing local registration without holding a scope borrow
    /// during user hashing or equality. Matching local identities bypass this.
    pub(super) fn take_claim(&self, key: &dyn ViewKey, owner: OwnerTag) -> Option<ScopedKeyOwner> {
        let hash = key.key_hash();
        let mut retried = false;
        loop {
            let snapshot = self.snapshot(hash);
            let identity = snapshot
                .iter()
                .find(|claim| claim.key.as_ref().key_eq(key))
                .map(|claim| claim.identity.clone());
            let state = self.state.borrow();
            if !state.matches_snapshot(hash, &snapshot) {
                drop(state);
                if retried {
                    refuse_unstable_comparison();
                }
                retried = true;
                retire_snapshot(snapshot);
                continue;
            }
            drop(state);
            let removed = identity
                .filter(|identity| identity.owner == owner)
                .and_then(|identity| self.take_identity(&identity));
            retire_snapshot(snapshot);
            return removed;
        }
    }

    fn take_identity(&self, identity: &ClaimIdentity) -> Option<ScopedKeyOwner> {
        self.take_claim_by_marker(identity.hash, identity.owner, &identity.marker)
    }

    /// Mandatory rollback uses only framework identity, never key callbacks.
    fn take_claim_by_marker(
        &self,
        hash: u64,
        owner: OwnerTag,
        marker: &Rc<()>,
    ) -> Option<ScopedKeyOwner> {
        let mut state = self.state.borrow_mut();
        let bucket = state.claims.get_mut(&hash)?;
        let index = bucket
            .iter()
            .position(|claim| claim.owner == owner && Rc::ptr_eq(&claim.marker, marker))?;
        let removed = bucket.swap_remove(index);
        if bucket.is_empty() {
            state.claims.remove(&hash);
        }
        drop(state);
        Some(removed.key)
    }

    /// Extract every claim tagged to `owner` without retiring arbitrary keys.
    ///
    /// Called from [`BuildOwner`](super::BuildOwner)'s `Drop` impl so a
    /// dropped owner's stale claims cannot wedge the scope forever. Asserts
    /// nothing — an owner-drop reclaim is expected background cleanup, not a bug:
    /// an owner dropped with zero live claims (the common case — every key
    /// was already unregistered through the normal unmount path) reclaims
    /// silently. The caller retires keys after releasing its binding guard.
    pub(crate) fn take_owner_claims(&self, owner: OwnerTag) -> Vec<ScopedKeyOwner> {
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
/// own local-map publication. Its identity comparisons can panic, so the guard
/// exists so that invariant is enforced by construction rather than by
/// caller discipline: anything landing between a future claim and its
/// commit that unwinds (a build error, a panic) still leaves the scope
/// claim-free, never a leaked claim nobody can ever release.
#[derive(Debug)]
pub(crate) struct ClaimGuard<'a> {
    scope: &'a GlobalKeyScope,
    owner: OwnerTag,
    claim: Option<(u64, Rc<()>)>,
    identity: ClaimIdentity,
}
impl ClaimGuard<'_> {
    /// The local registration is now published; rollback is disarmed.
    pub(crate) fn commit(mut self) {
        self.claim = None;
    }
}
impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        if let Some((hash, marker)) = self.claim.take() {
            let removed = self.scope.take_claim_by_marker(hash, self.owner, &marker);
            // Borrow released: the key slot preserves the incoming unwind.
            drop(removed);
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
    let prepared = local.prepare(key);
    match scope_ref.try_claim(key, owner) {
        Ok(guard) => {
            local.insert_prepared(key, element, prepared, guard.identity.clone());
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
    let local_key = local.take_registration(key);
    let scope_key = scope.and_then(|scope| {
        if let Some((_, _, identity)) = &local_key {
            (identity.owner == owner)
                .then(|| scope.take_identity(identity))
                .flatten()
        } else {
            scope.take_claim(key, owner)
        }
    });
    // Both authorities committed, with no scope borrow held during retirement.
    // Preserve ordinary local-before-scoped-key destruction order.
    drop(local_key);
    drop(scope_key);
}

#[cfg(test)]
impl OwnerTag {
    pub(crate) fn exhausted_owner_tag_counter_preserves_claim_authority() {
        let counter = AtomicU64::new(u64::MAX - 1);
        let previous = Self::fresh_with_counter(&counter);
        let last = Self::fresh_with_counter(&counter);
        let key = crate::GlobalKey::<()>::new();
        let scope = GlobalKeyScope::new();
        let mut shared = Some(scope.clone());
        let mut previous_local = GlobalKeyRegistry::new();
        let mut last_local = GlobalKeyRegistry::new();
        claim_and_register(
            &mut shared,
            previous,
            &key,
            ElementId::new(1),
            &mut previous_local,
        );
        release_and_unregister(Some(&scope), previous, &key, &mut previous_local);
        claim_and_register(&mut shared, last, &key, ElementId::new(2), &mut last_local);

        for _ in 0..8 {
            let refusal = std::panic::catch_unwind(|| Self::fresh_with_counter(&counter))
                .expect_err("exhaustion must refuse owner admission, never issue a stale tag");
            let message = refusal
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| refusal.downcast_ref::<&str>().copied())
                .expect("owner admission refusal carries an ordinary panic message");
            assert!(message.starts_with("OwnerTag counter exhausted:"));
            assert_eq!(counter.load(Ordering::Relaxed), 0);

            // A retired owner cannot withdraw the replacement's authority,
            // including after repeated caught admission failures.
            release_and_unregister(Some(&scope), previous, &key, &mut previous_local);
            let conflict = scope
                .try_claim(&key, previous)
                .expect_err("the final admitted owner must still hold the key");
            assert_eq!(conflict.holder, last);
            assert_eq!(last_local.get(&key), Some(ElementId::new(2)));
        }
        release_and_unregister(Some(&scope), last, &key, &mut last_local);

        // A separate healthy allocator and scope still admit distinct owners
        // and permit the next mount after matching release.
        let healthy = AtomicU64::new(1);
        let first = Self::fresh_with_counter(&healthy);
        let second = Self::fresh_with_counter(&healthy);
        let mut healthy_scope = Some(GlobalKeyScope::new());
        let mut healthy_local = GlobalKeyRegistry::new();
        claim_and_register(
            &mut healthy_scope,
            first,
            &key,
            ElementId::new(3),
            &mut healthy_local,
        );
        let conflict = healthy_scope
            .as_ref()
            .expect("healthy mount installs its scope")
            .try_claim(&key, second)
            .expect_err("consecutive healthy owners must have distinct authority");
        assert_eq!(conflict.holder, first);
        release_and_unregister(healthy_scope.as_ref(), first, &key, &mut healthy_local);
        claim_and_register(
            &mut healthy_scope,
            second,
            &key,
            ElementId::new(4),
            &mut healthy_local,
        );
        assert_eq!(healthy_local.get(&key), Some(ElementId::new(4)));
        release_and_unregister(healthy_scope.as_ref(), second, &key, &mut healthy_local);
    }
}
