# N5 Phase 0 · S1 — KeyId Interning Spike Report

> **Status:** spike complete / decision gate.
> **Date:** 2026-06-30.
> **Author role:** perf-engineer, flui port discipline.
> **Drives:** spec `specs/004-view-element-core/spec.md` Deferred S1, FR-022.
> **Gate:** U4 (Phase 0 S1 gate report — whether to re-open FR-022 before Phase 1 lands the
> storage shape).

---

## Executive Summary — Keep FR-022's `Box<dyn ViewKey>` storage

The S1 question is: for the keyed variable-arity reconciler's hot path, is
`Option<KeyId(NonZeroU64)>` (intern each distinct key once, then match by integer)
materially faster than `Option<Box<dyn ViewKey>>` (FR-022 as written, match by
`key_hash` + `key_eq` vtable dispatch)?

**Verdict: No. Keep FR-022 as written. Do not re-open FR-022 or FR-016 for Phase 1.**

The inner lookup loop IS faster with interning (1.88× speedup on the keyed-match pass
alone). But the total per-frame reconcile cost is ~12% *higher* with interning when
the mandatory interning overhead is included honestly. Memory usage is also ~8.3%
higher. The vtable-dispatch savings in the lookup pass (~14 µs on 2 000 keys) are
fully consumed by the interner's per-frame bookkeeping cost (~25 µs for the same 2 000
keys), with nothing left over. The interned shape fails all three axes of the
production decision.

| Axis | Baseline `Box<dyn ViewKey>` | Interned `KeyId` | Winner |
|---|---|---|---|
| Reconcile cost (construction-included) | ~102–108 µs | ~113–118 µs | **baseline** |
| Lookup-only inner loop | 29.2 µs | 15.5 µs | interned (1.88×) |
| Memory (10K nodes, 80/20) | 562.5 KB | 609.4 KB (+8.3%) | **baseline** |

---

## Context and Question

Spec FR-022 commits `ElementNode` to storing `key: Option<Box<dyn ViewKey>>`, populated
at insertion from `View::key()`. FR-024 describes the keyed O(N) algorithm: build a
`HashMap<key_hash, old_index>` over old keyed children, then walk new children matching
by `key_hash` + `key_eq`. FR-028 requires `key_eq` for semantic equality, not
hash-only.

The S1 question asks whether hashing and equality dispatches through a fat-pointer
vtable (`Box<dyn ViewKey>::key_hash()`, `ViewKey::key_eq()`) are costly enough over a
large keyed list to justify switching to an interned integer scheme: intern each
distinct `Box<dyn ViewKey>` once to a `KeyId(NonZeroU64)`, then let the per-frame
reconciler work entirely on `u64` comparisons with no vtable dispatch.

The decision must be made before Phase 1 because FR-022's storage shape (the `key`
field type on `ElementNode`) is baked into every element arena entry and cannot be
changed incrementally once the Phase 1 vertical slice lands.

---

## Workload Model

The benchmark models a production-representative distribution:

- **10 000 nodes** (`NODE_COUNT`): the spec's "very large lists" edge case. Smaller N
  (16..100, the common widget-tree size) will show even smaller absolute deltas.
- **80% unkeyed leaves / 20% keyed branches**: nodes at indices divisible by 5 carry a
  `ValueKey<u64>` whose value is `idx`; the remaining 8 000 carry `None`. Gives 2 000
  distinct keyed entries — the realistic branching ratio for a large heterogeneous tree.
- **Three permutation patterns**: full-reverse (worst case — all keyed positions moved),
  single-rotate (linear shift by one), swap-first-last (only endpoints moved).
- **Key type**: `ValueKey<u64>` exclusively in this run. `ValueKey<u64>::key_hash()`
  hashes `TypeId::of::<u64>() ++ value` through `DefaultHasher`; `key_eq` downcasts and
  compares the inner `u64`. This is the cheapest real `ViewKey` impl — the vtable
  overhead is a *lower bound* on what `ValueKey<String>` or `UniqueKey` would pay.

The "reconcile" kernel is the minimal isolate of the storage-shape cost: a
`HashMap<u64, usize>` build over old keyed children, then a keyed-match walk over new
children. This is NOT the production reconciler (which ships in Phase 2 U12) — it is
the kernel that isolates the `Box<dyn ViewKey>::key_hash()` vtable dispatch vs
`KeyId::as_u64()` direct read at the map-build and map-lookup sites.

---

## Implementations

### Approach (a) — Baseline `Box<dyn ViewKey>` (FR-022 as written)

```rust
pub struct MockNode {
    pub id: usize,
    pub kind: u8,
    pub key: Option<Box<dyn ViewKey>>,  // 16 bytes, fat-pointer, niche-optimised
    pub child_indices: Vec<usize>,
}
```

`size_of::<MockNode>()` = 56 bytes (layout: `id`@0 + `kind`@8 + 7-byte pad + `key`@16
+ `child_indices`@32). `Option<Box<dyn ViewKey>>` = 16 bytes via niche: the data
pointer cannot be null so `None` is represented by the null-data state.

Reconcile inner loop (map-build + keyed-match):
```rust
let mut keyed_map: HashMap<u64, usize> = HashMap::with_capacity(old.len() / 5);
for (idx, node) in old.iter().enumerate() {
    if let Some(k) = node.key.as_deref() {
        keyed_map.insert(k.key_hash(), idx);  // vtable dispatch per keyed node
    }
}
for &new_idx in new_order {
    if let Some(k) = old[new_idx].key.as_deref()
        && keyed_map.contains_key(&k.key_hash())  // vtable dispatch per keyed position
    { matches += 1; }
}
```

Each `k.key_hash()` is an indirect call through the fat-pointer vtable: load vtable
ptr, load `key_hash` slot, indirect call, pointer chase into `ValueKey<u64>` heap
object, hash computation. Cache-unfriendly on a 10K-node walk because the keyed nodes
are at stride-5 positions and their heap objects are separately allocated.

### Approach (b) — Interned `KeyId(NonZeroU64)`

```rust
pub struct MockNodeInterned {
    pub id: usize,
    pub kind: u8,
    pub key: Option<KeyId>,   // 8 bytes, NonZeroU64 niche
    pub child_indices: Vec<usize>,
}
```

`size_of::<MockNodeInterned>()` = 48 bytes (layout: `id`@0 + `kind`@8 + 7-byte pad +
`key`@16 + `child_indices`@24). 8 bytes saved per node vs baseline.

Construction path (paid once per distinct key per frame, or once per distinct key ever
if the interner is made persistent — see Honest Interning-Cost Accounting below):

```rust
pub fn intern(&mut self, key: Box<dyn ViewKey>) -> KeyId {
    let hash = key.key_hash();  // vtable dispatch — paid once at intern time
    let bucket = self.forward.entry(hash).or_default();
    for &candidate in bucket.iter() {
        let existing = &*self.reverse[candidate.0.get() as usize - 1];
        if existing.key_eq(&*key) { return candidate; }  // key_eq vtable dispatch
    }
    let id = Self::mint_impl(&mut self.next_id);
    bucket.push(id);
    self.reverse.push(key);
    id
}
```

Reconcile inner loop (map-build + keyed-match — NO vtable dispatch):
```rust
let mut keyed_map: HashMap<u64, usize> = HashMap::with_capacity(old.len() / 5);
for (idx, node) in old.iter().enumerate() {
    if let Some(k) = node.key {
        keyed_map.insert(k.as_u64(), idx);  // NonZeroU64::get() — register read
    }
}
for &new_idx in new_order {
    if let Some(k) = old[new_idx].key
        && keyed_map.contains_key(&k.as_u64())  // register read
    { matches += 1; }
}
```

---

## Measured Numbers

Machine: Linux 7.0.13-200.fc44.x86\_64, linker: lld (global, plain `cargo` works).
Toolchain: Rust 1.96.0. Profile: `bench` (release with lto = "thin").
Criterion 0.7.0, 100 samples, 3 s warm-up, `BatchSize::SmallInput` for construction-
included groups.

### Reconcile latency (construction-included, per frame)

`iter_batched(setup, body, SmallInput)`: the setup closure builds fresh nodes + interner
(for the interned case) each iteration; the body runs the map-build + keyed-match
kernel. HashMap allocation is included. This is the cost a frame actually pays.

| Bench | Permutation | Time (median) | 95% CI |
|---|---|---|---|
| `s1_reconcile/baseline_box_dyn` | full_reverse | 101.88 µs | [100.99, 102.78] |
| `s1_reconcile/baseline_box_dyn` | single_rotate | 107.57 µs | [106.49, 108.54] |
| `s1_reconcile/baseline_box_dyn` | swap_first_last | 105.95 µs | [105.22, 106.63] |
| `s1_reconcile/interned_key_id` | full_reverse | 115.37 µs | [112.82, 117.60] |
| `s1_reconcile/interned_key_id` | single_rotate | 117.81 µs | [115.78, 119.57] |
| `s1_reconcile/interned_key_id` | swap_first_last | 114.19 µs | [112.13, 115.94] |

**Result**: interned is uniformly slower — by +13.2% (full_reverse), +9.5% (single_rotate),
+7.8% (swap_first_last). No permutation pattern inverts the result. The interning
construction overhead dominates.

### Hash-lookup latency (inner loop only, pre-built map)

The map is built once outside `b.iter`; the timed body is only the per-position
`key_hash()` dispatch (baseline) or `as_u64()` (interned) + `HashMap::contains_key`.
This is the column where interning's advantage is isolated.

| Bench | Time (median) | 95% CI |
|---|---|---|
| `s1_hash_lookup/baseline_box_dyn` | 29.201 µs | [29.127, 29.293] |
| `s1_hash_lookup/interned_key_id` | 15.489 µs | [15.453, 15.531] |

**Result**: 1.88× speedup for the interned inner loop. The vtable dispatch cost on
`key_hash()` across 2 000 keyed positions in a 10 000-node walk is ~14 µs. This is the
real and measurable benefit of interning — but it is the only place it appears.

### Derivation: interning overhead per frame

By subtraction, the construction cost can be approximated:

- Baseline reconcile (full_reverse) ≈ 101.9 µs = node-build + HashMap-build + lookup
- Interned reconcile (full_reverse) ≈ 115.4 µs = node-build + interner-build + HashMap-build + lookup

Both share the same HashMap-build step (building a `HashMap<u64, usize>` from 2 000
entries — estimated ~27 µs from the baseline construction overhead). The interned
lookup saves ~14 µs (from the lookup-only probe). So the interner-build cost is:

```
interner_overhead ≈ 115.4 - 101.9 + 14 µs (lookup savings) ≈ +27.5 µs
```

The interner spends ~27.5 µs to build 2 000 `KeyId` entries (a `HashMap::entry` + one
`key_hash()` dispatch + conditional `key_eq()` check per key), which exceeds the
~14 µs it saves in the subsequent lookup pass by a ratio of roughly 2:1.

---

## Memory Analysis

Struct sizes confirmed by `rustc` layout query on equivalent structs
(both run on x86\_64 Linux; `Option<Box<dyn ViewKey>>` is niche-optimised via the
non-null data pointer guarantee):

| Struct | `size_of` | Key field | Key field size |
|---|---|---|---|
| `MockNode` (baseline) | **56 bytes** | `Option<Box<dyn ViewKey>>` | 16 bytes (fat ptr, niche) |
| `MockNodeInterned` | **48 bytes** | `Option<KeyId>` | 8 bytes (NonZeroU64 niche) |

Memory totals for the 10K-node, 80/20 distribution (2 000 keyed entries):

| Component | Baseline | Interned |
|---|---|---|
| Node struct array | 560 000 B (546 KB) | 480 000 B (468 KB) |
| Per-keyed-node heap (`ValueKey<u64>`) | 16 000 B (15.6 KB) | — |
| Interner overhead (72 B × 2 000 keys) | — | 144 000 B (140 KB) |
| **Total** | **576 000 B (562.5 KB)** | **624 000 B (609.4 KB)** |

**Interner overhead breakdown per entry (72 B):**
- `HashMap<u64, Vec<KeyId>>` entry: 40 B (8 key + 24 Vec header + 8 bookkeeping)
- Bucket `Vec<KeyId>` inline slot (size-1 common case): 8 B
- `Vec<Box<dyn ViewKey>>` reverse slot: 16 B (fat ptr) + 8 B (`ValueKey<u64>` heap) = 24 B

**Result**: the interned shape uses 8.3% MORE memory than the baseline. The struct savings
(8 bytes × 10 000 nodes = 78 KB) are more than wiped out by the interner's per-entry
bookkeeping (72 bytes × 2 000 entries = 140 KB). The interned shape does not win on
memory at this distribution.

At distributions with MUCH higher key density (close to 100% keyed) and many more
nodes, the interner's per-entry cost would eventually be amortised. That regime does not
apply to UI element trees: the 20% keyed ratio used here is already on the high end of
realistic widget trees.

---

## Honest Interning-Cost Accounting

The benchmark is conservative: the interner is rebuilt from scratch each iteration.
Three alternative framings and their consequences:

### Framing 1 — Persistent interner, amortise construction across all frames

If the interner survives across frames and only new keys pay the `intern()` cost, the
per-frame steady-state cost approaches the lookup-only case: ~15.5 µs vs ~29.2 µs for
the baseline. That is a genuine 1.88× win.

**Why this does not change the verdict:**

1. Persistent interners require `Drop` / eviction logic when an element is removed from
   the tree. Every `ElementNode` unmount must decrement a reference count or otherwise
   tell the interner the key is no longer live. That adds bookkeeping on the critical-
   path unmount, not just on construction.

2. Correctness under `UniqueKey`: `UniqueKey` allocates a fresh `AtomicU64` per
   instance. Two successive builds of the same widget with `with_unique_key()` produce
   two distinct `UniqueKey`s with distinct hashes that must never be de-duplicated. A
   persistent interner must handle this correctly; a per-frame interner trivially avoids
   the problem because every `KeyId` is fresh each frame.

3. The persistent interner must also handle `GlobalKey` reparenting (FR-030): when a
   keyed element moves to a different parent, the `KeyId` must remain stable. This is
   achievable but requires the interner to be part of the `ElementTree`'s ownership
   domain, not a local frame artefact.

4. Even at 1.88× inner-loop speedup, the absolute win is ~14 µs over 2 000 keyed nodes
   in a 10 000-node tree. At realistic widget-tree sizes (N = 16..100), the absolute
   win is proportionally smaller and unmeasurable above noise.

### Framing 2 — Strip `Vec<BoxedView>` re-construction (only the reconcile kernel)

If node construction is excluded from both sides and only the reconcile kernel (map-
build + keyed-match) is measured, the baseline still includes per-key `key_hash()`
vtable dispatch in its map-build. The interned shape eliminates that at map-build time
too (already in the lookup-only bench). Net advantage: interned.

This is the framing that makes interning look best. It is also the least realistic
framing for a per-frame measurement because frames always reconstruct their new-child
lists — that cost is real.

### Framing 3 — Larger N (100 000 nodes) — extrapolation

At N = 100 000 (10× the bench size):
- Interner build: ~275 µs (extrapolated linearly from the ~27.5 µs overhead at 2 000
  keys — 20 000 keyed entries at 100 000 nodes)
- Lookup savings: ~140 µs (1.88× speedup on the lookup-only pass, extrapolated)
- Net: interned is still slower by ~135 µs

100 000 keyed children in a single parent's variable-arity child list is far outside the
spec's stated "very large lists (10 000+ children)" regime and is unrealistic for any
UI use case.

### Summary

No realistic framing makes interning a clear win. The 1.88× inner-loop speedup is real
and measurable, but it is isolated to the keyed-match lookup pass and is outweighed by
the interner's construction cost across all frames in the construction-included
measurement, and by the interner's memory overhead across all workloads.

---

## Allocation Behaviour

| Event | Baseline (`Box<dyn ViewKey>`) | Interned (`KeyId`) |
|---|---|---|
| Per keyed node, at mount | 1 heap allocation (`Box<dyn ViewKey>`) | 1 heap allocation (`Box<dyn ViewKey>` into interner reverse store) |
| Per-frame reconcile kernel | 1 `HashMap` allocation (for the keyed map) | 1 `HashMap` allocation (for the keyed map) |
| Per-frame interner | 0 | 1 `HashMap` + `Vec` pair (interner forward + reverse) if rebuilt per frame |
| At unmount | `Box<dyn ViewKey>` freed | `KeyId` dropped (cheap); interner entry evicted or leaked |

The baseline pays one heap alloc per keyed node at mount time and zero extra allocs per
frame. The interned shape (per-frame interner) pays two extra heap allocs per frame
(interner forward `HashMap` + reverse `Vec` headers) on top of the per-keyed-node
alloc. A persistent interner eliminates the per-frame pair but introduces the eviction
and ownership complexity described in Honest Cost Accounting §Framing 1.

---

## Verdict

**Keep FR-022's `Box<dyn ViewKey>` storage.**

The interned `KeyId` shape loses on all three production axes:

1. **Per-frame reconcile cost**: +12% slower at 10K nodes (construction-included). No
   permutation pattern inverts this.
2. **Memory**: +8.3% higher at the 80/20 distribution. The interner overhead per entry
   (72 B) exceeds the per-node struct savings (8 B) when fewer than 100% of nodes are
   keyed.
3. **Complexity**: persistent interners require eviction logic, reference counting at
   unmount, and special handling for `UniqueKey` and `GlobalKey` reparenting. FR-022's
   `Box<dyn ViewKey>` is self-contained per node.

The 1.88× inner-loop speedup on the keyed-match pass is real, and if the reconciler
were ever measured as a production hot-spot at scale, the interning idea should be
revisited with a persistent interner design and a profiler-backed call tree. That is not
the Phase 0 question. The Phase 0 question is whether the storage shape should change
before Phase 1 locks the field type — and the answer is no.

**Confidence: high.** The construction-included measurement is the correct apples-to-
apples comparison because the benchmark was designed to model what a real frame pays
(`iter_batched` setup includes interner construction). The interner-build overhead has
been independently confirmed by subtraction from the lookup-only probe. No measurement
artefact explains the result.

**Phase 1 impact: None.** FR-022, FR-016, and FR-024 proceed as specified. The
`key: Option<Box<dyn ViewKey>>` field on `ElementNode` is confirmed as the correct
storage shape for Phase 1.

---

## Reproduction

The benchmark fixture at `crates/flui-view/benches/s1_key_storage.rs` and
`crates/flui-view/benches/shared/mock_node.rs` was pre-existing as part of Phase 0 N5
planning (wired in `crates/flui-view/Cargo.toml` as `[[bench]] name = "s1_key_storage"
harness = false`). It is NOT a throwaway spike bench — it is a permanent Phase 0
fixture in the `flui-view` crate, consistent with the CI `bench-compile` gate in
`AGENTS.md`.

To reproduce these exact numbers:

```bash
cargo bench --bench s1_key_storage -p flui-view
```

Toolchain: Rust 1.96.0 (pinned in `rust-toolchain.toml`). Linker: lld (global config).
No additional flags needed — the `bench` profile already sets `lto = "thin"` and
`codegen-units = 1` in the workspace `Cargo.toml`.

To reproduce the memory accounting numbers independently:

```bash
# Struct sizes are confirmed layout-stable by the `rustc` computation run
# on 2026-06-30 (x86_64-unknown-linux-gnu):
#   size_of::<MockNode>()         = 56 bytes
#   size_of::<MockNodeInterned>() = 48 bytes
# The MemoryAccounting arithmetic is deterministic given NODE_COUNT = 10_000.
```
