//! Synthetic 16-position static-tuple mocks for the S2 algorithm sketch.
//!
//! This module is **bench-only** and **purpose-built for the S2 question** —
//! "given a true static-tuple `ViewSeq` path (`(A, B, C, ..., P)`), does a
//! compile-time-specialised reconciliation algorithm beat the linear keyed
//! algorithm by a material margin?". Inclusion is via the
//! `#[path = "shared/mock_tuple.rs"] mod mock_tuple;` attribute on the bench
//! file ([`s2_static_path.rs`]).
//!
//! Sibling [`mock_node.rs`] models the 10K-element dynamic-`Vec<BoxedView>`
//! distribution for the S1 storage-shape question; this module models the
//! 16-position static-tuple distribution for the S2 algorithm question. The
//! two mocks intentionally share **no types** — the S1 question is about
//! per-node storage shape over a large heterogeneous distribution, and the S2
//! question is about per-frame algorithm shape over a fixed 16-position tuple.
//! Sharing a node primitive between them would couple two unrelated bench
//! decisions; the plan's "if needed, add it to a NEW shared/mock_tuple.rs"
//! footnote prescribes this split.
//!
//! # The static-path observation
//!
//! In a true static-tuple `(A, B, C, ..., P)` setting, "reordering" means a
//! literally different type signature — `(C, A, B)` is not a permutation of
//! `(A, B, C)` at the type level; it is a different generic type. So the
//! keyed-reorder problem the linear algorithm solves
//! ([`mock_node::reconcile_baseline_keyed`]) is **structurally absent** at the
//! static path. The S2 question is: given that the linear keyed algorithm is
//! over-engineered for the static path, does a specialised algorithm produce
//! meaningfully better perf at the fixed 16-position grain?
//!
//! # Workload
//!
//! Fixed-arity 16 (per spec FR-013: `ViewSeq` tuple impls cap at `0..=16`).
//! Each position carries a synthetic [`TypeIdSlot`] — a `(TypeId, key_hash)`
//! pair modelling what a real `(A, B, ..., P)` tuple's per-position view would
//! expose to a per-position-comparing reconciler. The mock distribution mints
//! 16 distinct `TypeId` values via 16 distinct zero-sized marker types
//! (`Marker0`..`Marker15`); each position's `key_hash` is its `Marker{idx}`'s
//! `TypeId` hash.
//!
//! # Why two specialised variants
//!
//! The doc verdict's "is the specialised path materially faster" question has
//! two honest framings:
//!   1. **Positional-only specialised** — walk 16 positions, compare TypeIds
//!      per slot, emit Reuse/Replace. **O(N)**, no allocation, **no reorder
//!      detection** (this is the static-path-pure shape — reordering at the
//!      static path is a no-op because positions are compile-time-fixed).
//!   2. **Reorder-aware specialised** — walk old TypeIds into a
//!      `[Option<u8>; 16]` index map (stack-allocated, no `HashMap`), then walk
//!      new TypeIds matching against it. **O(N)** with O(1)-per-position
//!      lookup, no heap allocation. This is the apples-to-apples comparison
//!      against the linear keyed algorithm — same big-O, lower constant.
//!
//! Both variants are run; the doc's verdict tabulates both against the linear
//! baseline.
//!
//! # Plan / spec references
//!
//! - [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`]
//! - [`specs/004-view-element-core/spec.md`] Deferred S2, FR-016

use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

// ----------------------------------------------------------------------------
// Fixed-arity 16 — per spec FR-013 cap
// ----------------------------------------------------------------------------

/// Fixed tuple arity. Spec FR-013 caps `ViewSeq` tuple impls at `0..=16`; we
/// measure at the cap because that is the worst case the static-path
/// specialised algorithm pays for.
pub const TUPLE_ARITY: usize = 16;

// ----------------------------------------------------------------------------
// 16 marker types — one distinct TypeId per position
// ----------------------------------------------------------------------------

// Each marker is a zero-sized struct so its TypeId is a unique compile-time
// constant. 16 markers gives us 16 distinct positional types — exactly what
// a real `(A, B, ..., P)` tuple's positions would yield.
//
// We could have generated these with a macro; the explicit hand-written list
// stays bench-fixture-style readable, and the count is small enough that
// macro overhead would not pay back.
#[allow(dead_code)] // each marker's TypeId is consumed via TYPE_IDS, not the struct itself
pub struct Marker0;
#[allow(dead_code)]
pub struct Marker1;
#[allow(dead_code)]
pub struct Marker2;
#[allow(dead_code)]
pub struct Marker3;
#[allow(dead_code)]
pub struct Marker4;
#[allow(dead_code)]
pub struct Marker5;
#[allow(dead_code)]
pub struct Marker6;
#[allow(dead_code)]
pub struct Marker7;
#[allow(dead_code)]
pub struct Marker8;
#[allow(dead_code)]
pub struct Marker9;
#[allow(dead_code)]
pub struct Marker10;
#[allow(dead_code)]
pub struct Marker11;
#[allow(dead_code)]
pub struct Marker12;
#[allow(dead_code)]
pub struct Marker13;
#[allow(dead_code)]
pub struct Marker14;
#[allow(dead_code)]
pub struct Marker15;

/// Returns the 16 distinct `TypeId`s, one per `Marker{0..15}`. The bench
/// constructs old + new arrays from this slice and permutation patterns
/// derived from it.
///
/// Each TypeId is a compile-time constant per *The Rust Performance Book*
/// "TypeId" idiom — `TypeId::of::<T>()` is `const fn` since Rust 1.36, so the
/// call has zero per-iteration cost beyond the unconditional load.
#[must_use]
pub fn type_ids() -> [TypeId; TUPLE_ARITY] {
    [
        TypeId::of::<Marker0>(),
        TypeId::of::<Marker1>(),
        TypeId::of::<Marker2>(),
        TypeId::of::<Marker3>(),
        TypeId::of::<Marker4>(),
        TypeId::of::<Marker5>(),
        TypeId::of::<Marker6>(),
        TypeId::of::<Marker7>(),
        TypeId::of::<Marker8>(),
        TypeId::of::<Marker9>(),
        TypeId::of::<Marker10>(),
        TypeId::of::<Marker11>(),
        TypeId::of::<Marker12>(),
        TypeId::of::<Marker13>(),
        TypeId::of::<Marker14>(),
        TypeId::of::<Marker15>(),
    ]
}

// ----------------------------------------------------------------------------
// TypeIdSlot — minimal per-position payload
// ----------------------------------------------------------------------------

/// Per-position payload modelling what a real `(A, B, ..., P)` tuple's
/// position would expose to a per-position-comparing reconciler — a `TypeId`
/// (the position's compile-time type identity) and a derived `key_hash`. The
/// `key_hash` is computed once at slot-construction time so the per-iteration
/// hot path is a single u64 read, not a hasher run.
///
/// `#[repr(C)]` so the field layout is stable; the bench measures the
/// per-position payload cost as well as the algorithm shape.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TypeIdSlot {
    /// The position's TypeId — what a real `(A, B, ..., P)` tuple's
    /// `TypeId::of::<A>()` per position would yield.
    pub type_id: TypeId,
    /// Pre-computed hash of the `type_id`. Strips per-iteration hasher cost
    /// from the latency probes — the bench is measuring algorithm shape, not
    /// hasher throughput.
    pub key_hash: u64,
}

impl TypeIdSlot {
    /// Construct a slot from a `TypeId`. The `key_hash` is pre-computed via the
    /// stdlib `DefaultHasher` — same path the linear keyed reconciler uses to
    /// derive a `u64` from a `&dyn ViewKey::key_hash()` return value.
    #[must_use]
    pub fn new(type_id: TypeId) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        type_id.hash(&mut hasher);
        Self {
            type_id,
            key_hash: hasher.finish(),
        }
    }
}

// ----------------------------------------------------------------------------
// Permutation pattern — full reverse (worst case)
// ----------------------------------------------------------------------------

/// Apply a full-reverse permutation to the 16-slot array. The
/// canonical workload is the full-reverse 16-tuple — the worst case where
/// every position has moved, no prefix-scan or suffix-scan fast path can fire,
/// and every position pays the full algorithm cost.
#[inline]
pub fn full_reverse(slots: &mut [TypeIdSlot; TUPLE_ARITY]) {
    slots.reverse();
}

// ----------------------------------------------------------------------------
// Construction helpers
// ----------------------------------------------------------------------------

/// Build the identity 16-tuple slot array — slot `i` carries `Marker{i}`'s
/// TypeId. The "old" side of every bench iteration.
#[must_use]
pub fn identity_slots() -> [TypeIdSlot; TUPLE_ARITY] {
    let ids = type_ids();
    let mut out = [TypeIdSlot::new(ids[0]); TUPLE_ARITY];
    for (i, id) in ids.iter().enumerate() {
        out[i] = TypeIdSlot::new(*id);
    }
    out
}

/// Build the full-reverse 16-tuple slot array — the "new" side of every
/// bench iteration.
#[must_use]
pub fn reversed_slots() -> [TypeIdSlot; TUPLE_ARITY] {
    let mut s = identity_slots();
    full_reverse(&mut s);
    s
}

// ----------------------------------------------------------------------------
// Reconcile action — return shape, deliberately uniform across algorithms
// ----------------------------------------------------------------------------

/// Per-position outcome. The same shape used by all three algorithms so the
/// bench measures algorithm cost in isolation, not divergent return-type
/// construction cost.
///
/// - `Reuse(old_idx)` — the new position's type matched the old position's
///   type at `old_idx`. The element is preserved.
/// - `Replace` — no type match; a fresh element is created at this position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileAction {
    /// Reuse the old element at `old_idx` (may differ from the new position).
    Reuse(u8),
    /// No match found — create a fresh element at this position.
    Replace,
}

/// Fixed-arity 16 result array. `[ReconcileAction; TUPLE_ARITY]` is stack-only,
/// no heap allocation — exactly the "zero-cost specialised path" the doc's
/// verdict asks about.
pub type ReconcileResult = [ReconcileAction; TUPLE_ARITY];

// ----------------------------------------------------------------------------
// Algorithm A — linear keyed (the FR-016 baseline, simulated)
// ----------------------------------------------------------------------------

/// Linear keyed algorithm — the FR-016 baseline shape applied to the
/// 16-tuple case. Builds a `HashMap<u64, u8>` over old `key_hash` -> old
/// index, then walks new positions looking up by `key_hash`. This is the
/// **same shape** as the production keyed reconciler's keyed-map-build + walk
/// phases (`crates/flui-view/src/tree/id_reconcile.rs`), reduced to
/// the workload kernel.
///
/// O(N) average — the HashMap allocation and per-position hash dispatch are
/// the dominant per-frame costs.
///
/// Returns the per-position reconcile actions over the new array.
#[must_use]
#[inline(never)] // keeps the function call shape stable so the bench measures one
// body, not an inlined-into-caller variation
pub fn reconcile_linear_keyed(
    old: &[TypeIdSlot; TUPLE_ARITY],
    new: &[TypeIdSlot; TUPLE_ARITY],
) -> ReconcileResult {
    let mut keyed: HashMap<u64, u8> = HashMap::with_capacity(TUPLE_ARITY);
    for (i, slot) in old.iter().enumerate() {
        keyed.insert(slot.key_hash, i as u8);
    }
    let mut out = [ReconcileAction::Replace; TUPLE_ARITY];
    for (i, slot) in new.iter().enumerate() {
        if let Some(&old_idx) = keyed.get(&slot.key_hash) {
            out[i] = ReconcileAction::Reuse(old_idx);
        }
    }
    out
}

// ----------------------------------------------------------------------------
// Algorithm B — positional specialised (pure static-path shape)
// ----------------------------------------------------------------------------

/// Positional-only specialised algorithm — the **pure static-path shape**.
/// Walks 16 positions, compares `TypeId` per slot. If `old[i].type_id ==
/// new[i].type_id`, emit `Reuse(i)`; otherwise emit `Replace`. **No
/// cross-position lookup**, because at the true static-tuple path positions
/// are compile-time-fixed and "the type at position i" is a property of the
/// generic parameter, not a runtime value to be searched.
///
/// O(N), no heap allocation, no HashMap. Per *The Rust Performance Book*
/// "monomorphisation" idiom this is the shape a real
/// `const fn reconcile_tuple_16<A, B, ..., P>(...)` would compile to in the
/// limit — the per-position TypeId comparison is the only operation.
///
/// **What this algorithm cannot do**: it cannot detect cross-position
/// reordering. If the tuple type signature changes (`(A, B, C)` -> `(C, A, B)`),
/// every position reports `Replace` — but **that is correct** at the static
/// path, because at the static path a different tuple type is structurally a
/// different `ViewSeq` and the framework's positional reconciler is the right
/// answer (the keyed-state-preserving reorder is a `Vec<BoxedView>` concern,
/// not a tuple concern).
#[must_use]
#[inline(never)]
pub fn reconcile_positional_specialised(
    old: &[TypeIdSlot; TUPLE_ARITY],
    new: &[TypeIdSlot; TUPLE_ARITY],
) -> ReconcileResult {
    let mut out = [ReconcileAction::Replace; TUPLE_ARITY];
    for i in 0..TUPLE_ARITY {
        if old[i].type_id == new[i].type_id {
            out[i] = ReconcileAction::Reuse(i as u8);
        }
    }
    out
}

// ----------------------------------------------------------------------------
// Algorithm C — reorder-aware specialised (stack-allocated index map)
// ----------------------------------------------------------------------------

/// Reorder-aware specialised algorithm — the apples-to-apples comparison
/// against the linear keyed algorithm. Builds a stack-allocated
/// `[Option<u8>; TUPLE_ARITY]` index over `(key_hash mod TUPLE_ARITY)`-bucketed
/// old positions, then walks new positions looking up by `key_hash mod
/// TUPLE_ARITY`. On collision (two old positions hash to the same bucket),
/// the second falls into a linear probe over the array.
///
/// **Complexity:** O(N) average (size-1 buckets, immediate match), O(N²)
/// worst case under adversarial bucket collisions across all N positions.
/// Bounded to N = `TUPLE_ARITY` = 16 by FR-013, so the worst-case absolute
/// cost is capped at 256 probes per reconcile call — see the worst-case
/// timing table in `docs/research/2026-05-22-s2-static-path-sketch.md`.
///
/// The stack-allocated index is the structural win over the linear keyed
/// algorithm: no `HashMap::with_capacity(TUPLE_ARITY)` heap allocation per
/// frame, no hasher per lookup (the bench pre-computes `key_hash` once at slot
/// construction), no `Box<dyn ViewKey>` vtable dispatch.
///
/// **Semantic difference from Algorithm A**: same O(N) shape, same outputs
/// for any 16 distinct TypeIds; differs only on synthetic hash collisions
/// (which the spec's TypeId-derived hashes do not encounter at the 16-element
/// scale). This is the algorithm the doc's verdict measures against the
/// linear keyed baseline.
#[must_use]
#[inline(never)]
pub fn reconcile_reorder_specialised(
    old: &[TypeIdSlot; TUPLE_ARITY],
    new: &[TypeIdSlot; TUPLE_ARITY],
) -> ReconcileResult {
    // Stack-allocated bucket index. `Option<u8>` is 2 bytes (1 byte tag +
    // 1 byte payload — no niche since u8 has no reserved value); 16 slots =
    // 32 bytes, all stack-resident, zero heap allocation. The shape choice
    // is "small and stack-bound" not "niche-packed"; the latter would need
    // `Option<NonZeroU8>` with a +1-bias index encoding which trades 0 bytes
    // of memory for code complexity not worth it at TUPLE_ARITY = 16.
    let mut index: [Option<u8>; TUPLE_ARITY] = [None; TUPLE_ARITY];
    for (i, slot) in old.iter().enumerate() {
        // Direct-hash bucketing — `key_hash` is already a u64; modulo
        // collapses it into the 16-slot index. Linear probe on collision.
        // Bounded by TUPLE_ARITY (matches NEW-side bound at line 401) so a
        // future change that depopulates `index` between iterations cannot
        // produce an unterminated probe. Sound by pigeonhole at TUPLE_ARITY
        // slots / TUPLE_ARITY inserts, but the explicit bound matches the
        // NEW-side symmetry and survives signature refactors.
        let mut bucket = (slot.key_hash as usize) % TUPLE_ARITY;
        let mut probes = 0;
        while probes < TUPLE_ARITY {
            if index[bucket].is_none() {
                index[bucket] = Some(i as u8);
                break;
            }
            bucket = (bucket + 1) % TUPLE_ARITY;
            probes += 1;
        }
    }

    let mut out = [ReconcileAction::Replace; TUPLE_ARITY];
    for (i, slot) in new.iter().enumerate() {
        let mut bucket = (slot.key_hash as usize) % TUPLE_ARITY;
        // Bound the probe loop to TUPLE_ARITY iterations per Codex review #6.
        // If `index` is fully populated (16 `Some` entries with no matching
        // key — possible when new keys are absent from old, e.g., any replace
        // scenario), the original loop's `Some(_) => keep probing` arm had no
        // termination. Bounded probe count guarantees worst-case full scan
        // before falling through to the default `Replace` action.
        let mut probes = 0;
        while probes < TUPLE_ARITY {
            match index[bucket] {
                Some(old_idx) if old[old_idx as usize].key_hash == slot.key_hash => {
                    out[i] = ReconcileAction::Reuse(old_idx);
                    break;
                }
                Some(_) => {
                    bucket = (bucket + 1) % TUPLE_ARITY;
                    probes += 1;
                }
                None => break,
            }
        }
    }
    out
}
