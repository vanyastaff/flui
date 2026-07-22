//! Synthetic `ElementNode` storage mocks for the key-storage-shape prototype bench.
//!
//! This module is **bench-only**. It must not be referenced from production code,
//! from `flui-view`'s `tests/` tree, or from other crates. Inclusion is via the
//! `#[path = "shared/mock_node.rs"] mod mock_node;` attribute on the bench file
//! ([`s1_key_storage.rs`]).
//!
//! # What this models
//!
//! This prototype must not modify production storage (per
//! [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`]).
//! We are measuring two candidate storage *shapes* for the `key` field on
//! `ElementNode` independently of the rest of the production lifecycle:
//!
//! - **Baseline** — `key: Option<Box<dyn ViewKey>>`. The shape spec FR-022
//!   commits to. `Option<Box<dyn ViewKey>>` is a fat-pointer 16 bytes
//!   (data pointer + vtable pointer) per `ElementNode`, with a heap allocation
//!   for every keyed node.
//! - **Interned** — `key: Option<KeyId>` where `KeyId = NonZeroU64`. Niche
//!   optimisation gives `Option<KeyId> = 8 bytes` per node. The heap cost
//!   relocates to a single shared interning table (`HashMap<u64, KeyId>`
//!   forward + `Vec<Box<dyn ViewKey>>` reverse) amortised across all nodes
//!   instead of paid per node.
//!
//! Both shapes carry the identical `id + kind + child_indices` payload so the
//! per-node `mem::size_of` delta is purely the `key` field swap.
//!
//! # Workload
//!
//! Synthetic 10K-element distribution at 80% unkeyed leaf / 20% keyed branch.
//! The "reconcile" workload is a simplified placeholder
//! (HashMap key-build + lookup + match count) — NOT the production reconciler,
//! which is implemented separately. We are measuring the relative cost of
//! `Box<dyn ViewKey>::key_hash()` dispatch vs `KeyId` direct u64 access at the
//! HashMap key-build site, on three canonical permutations (full-reverse,
//! single-rotate, swap-first-last).
//!
//! # Memory accounting
//!
//! Deterministic — the production allocator is not instrumented. For each
//! shape, the resident-bytes calculation sums:
//!   1. `mem::size_of::<MockNode<_>>() * N`
//!   2. heap-allocated `Box<dyn ViewKey>` cost per keyed node (baseline only)
//!   3. interning-table overhead (interned only, one-time)
//!
//! See [`MemoryAccounting`] for the public surface.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::struct_field_names)]

use std::collections::HashMap;
use std::num::NonZeroU64;

use flui_foundation::{ValueKey, ViewKey};

// ----------------------------------------------------------------------------
// Workload distribution
// ----------------------------------------------------------------------------

/// Fixed at 10K nodes. The bench scales workload size only via
/// permutation pattern — node count stays fixed so the memory column in the
/// gate report is comparable shape-to-shape.
pub const NODE_COUNT: usize = 10_000;

/// 80% unkeyed leaf, 20% keyed branch. A node is "keyed" iff its
/// index is a multiple of 5 — gives a deterministic 20/80 split without
/// random-number plumbing across iterations.
#[inline]
#[must_use]
pub fn is_keyed_index(idx: usize) -> bool {
    idx.is_multiple_of(5)
}

// ----------------------------------------------------------------------------
// KeyId — interned-shape primitive
// ----------------------------------------------------------------------------

/// Interned key identifier. `NonZeroU64` so `Option<KeyId>` is 8 bytes via
/// niche optimisation (per *The Rust Performance Book*, Memory chapter,
/// "niche optimization" idiom).
///
/// `KeyId` values are minted by [`KeyInterner`] from concrete `Box<dyn ViewKey>`
/// values at mock-construction time; the bench's per-frame hot path never
/// touches the underlying `Box<dyn ViewKey>` once the `KeyId` is assigned. That
/// is the measurement we want: how much faster does the reconciler's keyed-map
/// build run when each `key_hash()` is a free `NonZeroU64::get()` instead of a
/// vtable dispatch into the boxed `ViewKey` impl.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyId(NonZeroU64);

impl KeyId {
    /// The raw u64 backing this `KeyId`. Used as the HashMap hash in the
    /// interned-shape reconcile workload.
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0.get()
    }
}

// ----------------------------------------------------------------------------
// KeyInterner — Box<dyn ViewKey> -> KeyId interning table
// ----------------------------------------------------------------------------

/// Forward `key_hash -> KeyId` + reverse `KeyId -> Box<dyn ViewKey>` interning
/// table. One instance per bench iteration; the table is fully populated at
/// node-construction time and the reconcile workload reads `KeyId`s directly
/// without consulting it.
///
/// The reverse store is `Vec<Box<dyn ViewKey>>` (not `Slab`) because:
///   - `KeyId` is monotonically assigned (`next_id`) — no gaps.
///   - `Vec` is one fewer dependency for a bench-only fixture.
///   - The `Slab` shape would be the choice if the production interner ever
///     needed to evict — out of scope for this spec-validation question.
#[derive(Debug, Default)]
pub struct KeyInterner {
    /// Hash-to-bucket index. Bucket-per-hash with `key_eq` resolution on hit —
    /// mirrors the production FR-024 hash+eq discipline so the bench faithfully
    /// models the same contract. Hash-only dedup would silently merge distinct
    /// keys whose hashes collide, skewing the bench's memory accounting
    /// downward; bucket-per-hash with `key_eq` keeps the accounting honest.
    forward: HashMap<u64, Vec<KeyId>>,
    /// Reverse store: `KeyId.as_u64() - 1` indexes into this vec.
    reverse: Vec<Box<dyn ViewKey>>,
    /// Next id to mint. Starts at 1 so `NonZeroU64::new` always succeeds.
    next_id: u64,
}

impl KeyInterner {
    /// Construct an empty interner with `next_id` seeded to 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            forward: HashMap::with_capacity(NODE_COUNT / 5),
            reverse: Vec::with_capacity(NODE_COUNT / 5),
            next_id: 1,
        }
    }

    /// Intern a boxed `ViewKey`. Returns the existing `KeyId` if a key
    /// matching `key_eq` already exists in the hash bucket; otherwise mints
    /// a fresh one and appends to the bucket.
    ///
    /// Bucket-with-`key_eq` resolution (per spec FR-024 — distinct keys with
    /// the same hash must NOT silently merge). Hash collisions are rare in
    /// practice (the production `ViewKey` hashers are well-distributed), so
    /// buckets are typically size-1; the additional cost is negligible vs
    /// the fidelity gain over the bench-only naive hash-only dedup.
    ///
    /// Uses `HashMap::entry` so the miss path is a single hash lookup
    /// (*Programming Rust* 2nd ed, HashMap entry API).
    pub fn intern(&mut self, key: Box<dyn ViewKey>) -> KeyId {
        let hash = key.key_hash();
        let bucket = self.forward.entry(hash).or_default();
        for &candidate in bucket.iter() {
            // SAFETY of indexing: `KeyId` values are 1..=`next_id-1`
            // mapping to `reverse[id-1]`; `mint()` enforces this invariant.
            let existing = &*self.reverse[candidate.0.get() as usize - 1];
            if existing.key_eq(&*key) {
                return candidate;
            }
        }
        let id = Self::mint_impl(&mut self.next_id);
        bucket.push(id);
        self.reverse.push(key);
        id
    }

    /// Mint the next `KeyId`. Bench-only — the production interner would
    /// either reuse `Key::new()`'s atomic counter or carry its own.
    #[allow(dead_code)] // kept on the impl surface; intern() now uses mint_impl
    fn mint(&mut self) -> KeyId {
        Self::mint_impl(&mut self.next_id)
    }

    /// Split-borrow form of [`Self::mint`] that takes the counter directly so
    /// callers holding a `&mut Vec<KeyId>` borrow (e.g., `entry().or_default()`
    /// chain in [`Self::intern`]) can still mint without re-borrowing `self`.
    fn mint_impl(next_id: &mut u64) -> KeyId {
        let n = *next_id;
        *next_id += 1;
        // next_id starts at 1 and only increments; the bench creates at most
        // NODE_COUNT/5 = 2000 ids per iteration, far below u64::MAX. The
        // expect() guards an arithmetic invariant a future caller could break
        // (e.g., by seeding next_id at 0); a tripped message names the field
        // so the maintainer knows which invariant was violated.
        let nz = NonZeroU64::new(n)
            .expect("KeyInterner::next_id seeded at 0 — must start at 1 per ID-offset invariant");
        KeyId(nz)
    }

    /// Number of distinct interned keys. Documented part of the fixture's
    /// public surface — `MemoryAccounting::for_interned` recomputes the
    /// distinct-key count from the index distribution rather than reading
    /// this field so accounting stays a pure function of `NODE_COUNT`.
    #[inline]
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.reverse.len()
    }
}

// ----------------------------------------------------------------------------
// MockNode — baseline shape
// ----------------------------------------------------------------------------

/// `Box<dyn ViewKey>` baseline shape. `key` is a fat 16-byte
/// `Option<Box<dyn ViewKey>>` per node; keyed nodes additionally pay a heap
/// allocation for the boxed `ValueKey<u64>` payload.
///
/// `kind` carries the same 1-byte `ElementKind` discriminant the spec FR-020
/// closed enum will host, padded out by the alignment of the
/// following fields. We model it as a `u8` so the shape comparison is honest:
/// both `MockNode` and `MockNodeInterned` carry the same `kind` representation,
/// and the only field that differs is `key`.
#[derive(Debug)]
pub struct MockNode {
    /// Per-node id. `usize` not `ElementId` — the mock skips the production
    /// `NonZeroUsize` discipline because the bench never threads ids back
    /// through a real lifecycle.
    #[allow(dead_code)]
    pub id: usize,
    /// 0..=7 in the production enum.
    #[allow(dead_code)]
    pub kind: u8,
    /// `Option<Box<dyn ViewKey>>` — 16 bytes, fat pointer, heap-backed when
    /// `Some`. This is the baseline shape spec FR-022 commits to, pending
    /// this prototype's storage-shape verdict.
    pub key: Option<Box<dyn ViewKey>>,
    /// Position-based child slots. Empty for leaves; populated for branches.
    /// `Vec` so the mock matches the production `Variable`-arity shape — the
    /// keyed reconciler's hot path walks a `&[ElementId]` slice over this
    /// vector's contents.
    #[allow(dead_code)]
    pub child_indices: Vec<usize>,
}

impl MockNode {
    /// Construct a node for index `idx` per the 80%/20% distribution.
    /// Keyed nodes (idx % 5 == 0) carry a `ValueKey<u64>` whose value is `idx`
    /// so each keyed node hashes to a deterministic distinct bucket.
    pub fn make(idx: usize) -> Self {
        let key: Option<Box<dyn ViewKey>> = if is_keyed_index(idx) {
            Some(Box::new(ValueKey::<u64>::new(idx as u64)))
        } else {
            None
        };
        Self {
            id: idx,
            kind: 0,
            key,
            child_indices: Vec::new(),
        }
    }
}

// ----------------------------------------------------------------------------
// MockNodeInterned — interned shape
// ----------------------------------------------------------------------------

/// `Option<KeyId>` interned shape. `key` is 8 bytes via niche optimisation;
/// keyed nodes no longer carry a per-node heap allocation — the boxed payload
/// has been hoisted into the [`KeyInterner`].
#[derive(Debug)]
pub struct MockNodeInterned {
    #[allow(dead_code)]
    pub id: usize,
    #[allow(dead_code)]
    pub kind: u8,
    /// `Option<KeyId>` — 8 bytes via `NonZeroU64` niche.
    pub key: Option<KeyId>,
    #[allow(dead_code)]
    pub child_indices: Vec<usize>,
}

impl MockNodeInterned {
    /// Construct a node for index `idx` against the shared `interner`.
    pub fn make(idx: usize, interner: &mut KeyInterner) -> Self {
        let key: Option<KeyId> = if is_keyed_index(idx) {
            let boxed: Box<dyn ViewKey> = Box::new(ValueKey::<u64>::new(idx as u64));
            Some(interner.intern(boxed))
        } else {
            None
        };
        Self {
            id: idx,
            kind: 0,
            key,
            child_indices: Vec::new(),
        }
    }
}

// ----------------------------------------------------------------------------
// Permutation patterns
// ----------------------------------------------------------------------------

/// Three canonical permutations exercised by the reconcile workload.
///
/// The plan calls for at minimum full-reverse + single-rotate +
/// swap-first-last. We ship all three so each Criterion group emits three
/// scenarios and the per-permutation memory cost is held constant across
/// shapes.
#[derive(Clone, Copy, Debug)]
pub enum Permutation {
    /// `[A, B, C, ..., Z]` -> `[Z, ..., C, B, A]`. Worst case for the
    /// prefix-scan/suffix-scan fast paths — every position needs a keyed lookup.
    FullReverse,
    /// `[A, B, C, ..., Y, Z]` -> `[B, C, ..., Y, Z, A]`. Linear shift by one;
    /// the keyed middle absorbs the entire shift.
    SingleRotate,
    /// `[A, B, ..., Y, Z]` -> `[Z, B, ..., Y, A]`. Only the two endpoints
    /// shuffle; the keyed middle sees a two-position match.
    SwapFirstLast,
}

impl Permutation {
    /// All three permutations in stable order. `criterion_group!` iterates
    /// over this slice so adding a fourth permutation does not require
    /// touching the bench file.
    pub const ALL: &'static [Self] = &[Self::FullReverse, Self::SingleRotate, Self::SwapFirstLast];

    /// Human-readable id for the Criterion bench function name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FullReverse => "full_reverse",
            Self::SingleRotate => "single_rotate",
            Self::SwapFirstLast => "swap_first_last",
        }
    }

    /// Apply this permutation to a `0..n` index sequence in place.
    pub fn apply(self, indices: &mut [usize]) {
        match self {
            Self::FullReverse => indices.reverse(),
            Self::SingleRotate => indices.rotate_left(1),
            Self::SwapFirstLast => {
                if !indices.is_empty() {
                    indices.swap(0, indices.len() - 1);
                }
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Memory accounting
// ----------------------------------------------------------------------------

/// Deterministic memory accounting for the two storage shapes. The bench prints
/// the accounting at run time via the `bench_memory_summary` Criterion group
/// (formally not a bench — a single-iteration measurement printed via
/// `c.bench_function` so the value lands in the Criterion report).
#[derive(Debug, Clone, Copy)]
pub struct MemoryAccounting {
    /// Sum of `mem::size_of::<MockNode>() * NODE_COUNT`.
    pub node_struct_bytes: usize,
    /// Heap cost: per-node `Box<dyn ViewKey>` allocation, charged only to
    /// keyed nodes. Baseline pays this per-node; interned pays zero here.
    pub heap_key_bytes: usize,
    /// Interning-table overhead. Interned shape only; baseline is zero.
    pub interner_bytes: usize,
}

impl MemoryAccounting {
    /// Total resident bytes — the column the gate report compares shape-to-shape.
    #[inline]
    #[must_use]
    pub const fn total(&self) -> usize {
        self.node_struct_bytes + self.heap_key_bytes + self.interner_bytes
    }

    /// Account for the baseline `MockNode` distribution.
    ///
    /// Heap cost per keyed node = `size_of::<ValueKey<u64>>()` + the dyn-vtable
    /// dispatch table is shared across all `ValueKey<u64>` instances and not
    /// counted per-node. Allocator round-up to alignment is the only delta to
    /// real-world bytes; we report the un-rounded sum for shape comparability.
    #[must_use]
    pub fn for_baseline(node_count: usize) -> Self {
        let node_struct_bytes = core::mem::size_of::<MockNode>() * node_count;
        let keyed_count = (0..node_count).filter(|&i| is_keyed_index(i)).count();
        let heap_key_bytes = core::mem::size_of::<ValueKey<u64>>() * keyed_count;
        Self {
            node_struct_bytes,
            heap_key_bytes,
            interner_bytes: 0,
        }
    }

    /// Account for the interned `MockNodeInterned` distribution.
    ///
    /// Interner overhead = HashMap entry per distinct hash + per-id bucket
    /// slot + reverse-vec slot + heap-allocated `ValueKey<u64>` payload.
    /// Per-entry estimate (assuming no hash collisions — typical for the
    /// bench's well-distributed `ValueKey<u64>` hash):
    ///   - `HashMap<u64, Vec<KeyId>>` entry: 8 (u64 key) + 24 (Vec header:
    ///     ptr + len + cap) + 8 (bucket bookkeeping) = **40 bytes**
    ///   - Vec bucket inline storage: 1 KeyId per bucket × 8 bytes = **8 bytes**
    ///   - `Vec<Box<dyn ViewKey>>` reverse slot: 16 (fat ptr) +
    ///     `size_of::<ValueKey<u64>>() = 8 bytes` heap allocation = **24 bytes**
    ///
    /// Total per entry: 40 + 8 + 24 = **72 bytes**.
    ///
    /// The bucket Vec's inline allocation is amortised — most buckets are
    /// size 1, no separate heap alloc beyond the Vec header. The hashbrown
    /// per-entry overhead estimate (~40 bytes) is a hand-rolled approximation
    /// that may drift against stdlib internal layout changes; conservative
    /// upper bound suitable for the gate-report-level comparison.
    #[must_use]
    pub fn for_interned(node_count: usize) -> Self {
        let node_struct_bytes = core::mem::size_of::<MockNodeInterned>() * node_count;
        let keyed_count = (0..node_count).filter(|&i| is_keyed_index(i)).count();
        // Per-entry estimate with the `Vec<KeyId>` bucket-per-hash fix:
        //   - HashMap<u64, Vec<KeyId>> entry: 8 (key) + 24 (Vec header) + 8 (bookkeeping) = 40
        //   - bucket inline KeyId: 8 (size 1 in collision-free common case)
        //   - reverse Vec<Box<dyn ViewKey>> slot: 16 (fat ptr) + size_of::<ValueKey<u64>>() = 8
        let per_entry = 40 + 8 + 16 + core::mem::size_of::<ValueKey<u64>>();
        let interner_bytes = per_entry * keyed_count;
        Self {
            node_struct_bytes,
            heap_key_bytes: 0,
            interner_bytes,
        }
    }
}

// ----------------------------------------------------------------------------
// Reconcile workload (placeholder algorithm)
// ----------------------------------------------------------------------------

/// Build a `HashMap<key_hash, old_index>` over keyed old nodes and count
/// how many new nodes match by key hash. This is **not** the production
/// reconciler — it is the minimal kernel that
/// isolates the cost of `Box<dyn ViewKey>::key_hash()` dispatch vs `KeyId`
/// direct u64 read at the HashMap key-build site.
///
/// Returns the number of matched keyed positions. Black-boxed by the caller.
#[must_use]
pub fn reconcile_baseline_keyed(old: &[MockNode], new_order: &[usize]) -> usize {
    let mut keyed_map: HashMap<u64, usize> = HashMap::with_capacity(old.len() / 5);
    for (idx, node) in old.iter().enumerate() {
        if let Some(k) = node.key.as_deref() {
            keyed_map.insert(k.key_hash(), idx);
        }
    }
    let mut matches = 0usize;
    for &new_idx in new_order {
        if let Some(k) = old.get(new_idx).and_then(|n| n.key.as_deref())
            && keyed_map.contains_key(&k.key_hash())
        {
            matches += 1;
        }
    }
    matches
}

/// Interned counterpart of [`reconcile_baseline_keyed`]. Same kernel, same
/// HashMap shape, but the key is `KeyId::as_u64()` — a direct
/// `NonZeroU64::get()`, no vtable dispatch, no boxed payload touched.
#[must_use]
pub fn reconcile_interned_keyed(old: &[MockNodeInterned], new_order: &[usize]) -> usize {
    let mut keyed_map: HashMap<u64, usize> = HashMap::with_capacity(old.len() / 5);
    for (idx, node) in old.iter().enumerate() {
        if let Some(k) = node.key {
            keyed_map.insert(k.as_u64(), idx);
        }
    }
    let mut matches = 0usize;
    for &new_idx in new_order {
        if let Some(k) = old.get(new_idx).and_then(|n| n.key)
            && keyed_map.contains_key(&k.as_u64())
        {
            matches += 1;
        }
    }
    matches
}

// ----------------------------------------------------------------------------
// Construction helpers
// ----------------------------------------------------------------------------

/// Build a 10K-node baseline distribution. Returns the vec alone; the bench's
/// `iter_batched` shape consumes this per-iteration to avoid amortising
/// construction cost into the measurement.
#[must_use]
pub fn build_baseline_nodes() -> Vec<MockNode> {
    (0..NODE_COUNT).map(MockNode::make).collect()
}

/// Build a 10K-node interned distribution. Returns the vec + interner so the
/// bench can hold both alive across the iteration scope. The interner is
/// constructed inside this function so each iteration starts from a fresh
/// table — the bench measures construction-included cost of the interned
/// shape, not just the lookup path.
#[must_use]
pub fn build_interned_nodes() -> (Vec<MockNodeInterned>, KeyInterner) {
    let mut interner = KeyInterner::new();
    let nodes = (0..NODE_COUNT)
        .map(|i| MockNodeInterned::make(i, &mut interner))
        .collect();
    (nodes, interner)
}

/// Default new-order index buffer (identity permutation).
#[must_use]
pub fn identity_order() -> Vec<usize> {
    (0..NODE_COUNT).collect()
}

// ----------------------------------------------------------------------------
// Hash-lookup-only kernels (Codex review #5 fix)
// ----------------------------------------------------------------------------

/// Pre-build the keyed `HashMap<u64, usize>` from old baseline nodes. Called
/// once outside `b.iter` so the hash-lookup probe measures lookup latency in
/// isolation, NOT lookup + map-construction (which was the Codex review #5
/// finding on the original `s1_hash_lookup/*` probes).
#[must_use]
pub fn build_keyed_map_baseline(old: &[MockNode]) -> HashMap<u64, usize> {
    let mut m = HashMap::with_capacity(old.len() / 5);
    for (idx, node) in old.iter().enumerate() {
        if let Some(k) = node.key.as_deref() {
            m.insert(k.key_hash(), idx);
        }
    }
    m
}

/// Pre-build the keyed `HashMap<u64, usize>` from old interned nodes.
/// Symmetric to [`build_keyed_map_baseline`] for the `KeyId` shape.
#[must_use]
pub fn build_keyed_map_interned(old: &[MockNodeInterned]) -> HashMap<u64, usize> {
    let mut m = HashMap::with_capacity(old.len() / 5);
    for (idx, node) in old.iter().enumerate() {
        if let Some(k) = node.key {
            m.insert(k.as_u64(), idx);
        }
    }
    m
}

/// Lookup-only baseline kernel — assumes `map` was pre-built by
/// [`build_keyed_map_baseline`]. The `b.iter` body should call this so the
/// timed work is exactly: per-position `key_hash()` dispatch + `contains_key`,
/// without any per-iteration map construction.
#[must_use]
pub fn lookup_only_baseline(
    old: &[MockNode],
    map: &HashMap<u64, usize>,
    new_order: &[usize],
) -> usize {
    let mut matches = 0usize;
    for &new_idx in new_order {
        if let Some(k) = old.get(new_idx).and_then(|n| n.key.as_deref())
            && map.contains_key(&k.key_hash())
        {
            matches += 1;
        }
    }
    matches
}

/// Lookup-only interned kernel — symmetric to [`lookup_only_baseline`] for
/// the `KeyId` shape.
#[must_use]
pub fn lookup_only_interned(
    old: &[MockNodeInterned],
    map: &HashMap<u64, usize>,
    new_order: &[usize],
) -> usize {
    let mut matches = 0usize;
    for &new_idx in new_order {
        if let Some(k) = old.get(new_idx).and_then(|n| n.key)
            && map.contains_key(&k.as_u64())
        {
            matches += 1;
        }
    }
    matches
}
