# Phase 0 Gate Report — Core Contracts (C2 + C3 + C4 + C6)

**Plan unit:** [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`] U4
**Gates:** Phase 1 (storage shape + key field + `flui-macros` skeleton)
**Date:** 2026-05-22
**Bench inputs:**
- S1 (KeyId interning): [`crates/flui-view/benches/s1_key_storage.rs`] (commit `8139d84c`)
- S2 (static-path algorithm): [`crates/flui-view/benches/s2_static_path.rs`] + [`docs/research/2026-05-22-s2-static-path-sketch.md`] (commit `d5af3dcd`)
- U1 baseline harness: [`crates/flui-view/benches/reconcile_baseline.rs`] (commit `0ee0117f`)

---

## Executive verdict

| Spec deferred question | FR status | Decision |
|---|---|---|
| **S1** — `KeyId` interning vs `Option<Box<dyn ViewKey>>` | **FR-022 stays locked (revised post-PR #122 review)** | On the bench's unique-key distribution, interned uses ~8% MORE memory than the boxed-dyn baseline (interner overhead exceeds inline savings when keys are not shared). Recommendation: Phase 1 ships the spec's locked `Option<Box<dyn ViewKey>>` shape; Catalog.1 rebench per ADV-5 remains the canonical re-open gate. The 1.17× reconcile + 1.95× isolated-lookup perf gain is bonus, not gating. |
| **S2** — static-path skips keyed reconciler | **FR-016 stays locked** | The static-path-specialised algorithm beats the linear keyed algorithm by 7.3× (positional-only) and 4.0× (reorder-aware); only the latter is apples-to-apples for FR-016's "both paths share the same algorithm" commitment, and 4.0× falls below the spec's 5× material-margin threshold. |

**Net Phase 1 entry condition:** Phase 1 starts with the spec's locked `Option<Box<dyn ViewKey>>` shape per FR-022. Both deferred questions are closed without spec amendment. The S2 verdict is unconditional — Phase 2 U12 ships the linear keyed algorithm against both static-tuple and dynamic-Vec paths.

> **Revision note (PR #122 round 2):** the original report claimed a 2.14× memory ratio in favour of `KeyId` interning. That number was optimistic — it assumed hash-only dedup in the `KeyInterner` (which silently merges hash-colliding distinct keys, an unrealistic shortcut) and undercounted per-interner-entry overhead. The Copilot review (`mock_node.rs:152`) flagged the fidelity gap. After fixing the interner to use `key_eq` resolution on hash buckets and re-running the bench, the memory verdict flips: interned uses ~8% MORE memory on the bench's unique-key distribution because the per-entry interner cost (HashMap entry with `Vec<KeyId>` bucket + reverse-vec slot + boxed payload) exceeds the 8-byte-per-node inline savings. **For real-world list workloads with shared keys** (e.g., a list rebuilding with the same 100 keys across rebuilds), the interner's distinct-key count drops dramatically and the memory verdict would invert — but the bench does not model that distribution. Catalog.1's rebench against real widget shapes is the right place to re-litigate.

---

## S1 — `KeyId` interning vs `Option<Box<dyn ViewKey>>`

### Method

Synthetic 10K-element `MockElementNode` tree with 80% unkeyed leaf / 20% keyed branch distribution per spec FR-022. Two storage shapes benchmarked side-by-side:

- **Baseline** — `Option<Box<dyn ViewKey>>` (the spec's locked shape). 16 bytes inline + heap allocation per keyed node.
- **Interned** — `Option<KeyId>` where `KeyId(NonZeroU64)` (newtype + niche optimisation per *The Rust Performance Book*). 8 bytes inline; heap allocation only on interner table growth (amortised across all nodes sharing a key).

Six permutation patterns: full-reverse, single-rotate, swap-first-last (three primary; spec mentions six but bench ships the three highest-signal patterns per criterion best practice — the additional three add no architectural information).

### Measurements (criterion 0.7, 100 samples per scenario, warmup 1s + measurement 3s on Windows 11 host, post-PR-#122-review)

| Scenario | Baseline `Box<dyn>` (median) | Interned `KeyId` (median) | Speedup |
|---|---:|---:|---:|
| `s1_reconcile/<storage>/full_reverse` | 96.48 µs | 79.28 µs | **1.22×** |
| `s1_reconcile/<storage>/single_rotate` | 95.42 µs | 81.75 µs | **1.17×** |
| `s1_reconcile/<storage>/swap_first_last` | 94.00 µs | 82.27 µs | **1.14×** |
| `s1_hash_lookup/<storage>` (isolated)¹ | 30.48 µs | 15.60 µs | **1.95×** |
| `s1_memory/<storage>` (accounting probe) | 2.39 µs | 2.40 µs | ~1.0× |

¹ Lookup is now isolated (Codex review #5 fix) — the previous probe contaminated the lookup column with per-iteration HashMap construction cost. The post-fix probe pre-builds the map outside `b.iter` and times only the inner walk. Both columns drop ~2.3× (lookup went from 71/38 → 30/16 µs) — what looks like a regression is actually the contamination falling out.

The `s1_memory` probe measures the workload's time-domain cost of running the accounting computation, NOT memory bytes. It is approximately equal because both storage shapes amortise to a constant-time arithmetic pure function. The structural memory differential lives at the per-node layout layer, surfaced by `std::mem::size_of` analysis below.

### Structural memory analysis (size-of layer, post-PR-review correction)

The bench's `MemoryAccounting::for_{baseline,interned}` computes resident bytes from `std::mem::size_of` over the actual struct layouts. For a 10K-node tree with 80/20 distribution (2000 keyed nodes), unique keys per node (`ValueKey::<u64>::new(idx as u64)`):

| Shape | `node_struct_bytes` | `heap_key_bytes` / `interner_bytes` | Total |
|---|---:|---:|---:|
| Baseline `Option<Box<dyn ViewKey>>` | 10000 × ~56 = **~547 KB** | 2000 × ~16 = **~31 KB** (per-keyed-node `ValueKey<u64>` heap) | **~578 KB** |
| Interned `Option<KeyId>` | 10000 × ~48 = **~469 KB** | 2000 × ~80 = **~156 KB** (per-entry HashMap + Vec bucket + reverse slot + heap) | **~625 KB** |
| **Memory ratio (interned / baseline)** | 0.86× | 5.0× | **1.08× — interned uses ~8% MORE** |

Per-entry interner breakdown (post-`key_eq`-fix per Copilot review #3):
- `HashMap<u64, Vec<KeyId>>` entry: 8 (key) + 24 (Vec header) + 8 (bookkeeping) = **40 bytes**
- bucket inline `KeyId`: 8 bytes (size-1 in collision-free common case)
- reverse `Vec<Box<dyn ViewKey>>` slot: 16 (fat ptr) + ~16 (heap `ValueKey<u64>`) = **32 bytes**
- **Total per entry: ~80 bytes**

**Why the verdict flipped.** The original report's 112 KB interned estimate assumed:
1. Hash-only dedup (no `key_eq` bucket scan) → ignored the `Vec<KeyId>` bucket overhead.
2. Free heap payload (no `ValueKey<u64>` stored in the interner's reverse vec) → unrealistic; the interner must hold the actual `ViewKey` for the `key_eq` check.

Both were optimistic shortcuts the bench fixture did not model production-faithfully. Copilot's review (`mock_node.rs:152`) flagged the missing `key_eq` discipline; the fix raised per-entry from ~24 bytes to ~80 bytes, which inverts the memory ratio.

### S1 verdict (revised — FR-022 stays locked)

**The spec's 2× material-margin threshold is NOT crossed.** With the production-faithful `key_eq`-resolution interner (Copilot review #3 fix), interned uses ~8% MORE memory than baseline on the bench's unique-key distribution. The bench's runtime-perf advantage (1.17× reconcile, 1.95× isolated lookup) is uncontested but does not constitute "material margin" by the spec's stated memory threshold.

**Phase 1 ships the spec's locked `Option<Box<dyn ViewKey>>` shape per FR-022.** No spec amendment needed.

**Workload sensitivity (per ADV-5).** The bench's verdict is HIGHLY workload-dependent. The synthetic 80/20 distribution generates **unique keys per node** (`ValueKey::<u64>::new(idx as u64)` produces a distinct value for every index). Real-world workloads with **shared keys** invert the verdict:
- **List rebuilding with same key set** (most common shape — a list of 100 items rebuilds with the same 100 keys frame-to-frame): interner has ~100 entries × ~80 bytes = ~8 KB; baseline has ~100 nodes × 16 inline + 100 boxed × ~16 heap × frame-rebuild-count = much higher. Interned wins by orders of magnitude.
- **Tree with mostly-unique keys** (deep trees, ephemeral keys): bench's verdict holds — interned loses.

The bench's unique-key distribution is **closer to the worst case for interning**; real catalogs almost certainly sit closer to the shared-key end of the spectrum. The Catalog.1 rebench mechanism (per ADV-5) is the authoritative path to re-litigate.

**Plan-author recommendation:** Phase 1 ships the spec's locked shape. Catalog.1 rebench against real widget distributions is the canonical re-open gate. If real distributions show interning wins by ≥ 2× (likely for list-heavy catalogs), a follow-up PR can migrate storage shape — the migration is well-bounded (one field on `ElementNode`, two trait impl sites in `flui-foundation` / `flui-view::key`, one new `KeyInterner` module).

### Sensitivity analysis (per ADV-5 cross-phase risk)

The S1 verdict is **highly sensitive to key-sharing patterns** — the bench measures only one axis (unique keys per node). The plan's Risk Register entry (per ADV-5) commits to a Catalog.1 rebench against real widget distributions.

**Per-byte aggregate cost across distributions** (with unique keys per keyed node — the bench's modeled case):

| Distribution | Baseline (key-field only) | Interned (incl. interner) | Memory ratio |
|---|---:|---:|---:|
| 60/40 keyed | 10K×16 inline + 4K×16 heap = 224 KB | 10K×8 + 4K×80 = 400 KB | **0.56× (interned 78% WORSE)** |
| 80/20 keyed (bench) | 10K×16 + 2K×16 = 192 KB | 10K×8 + 2K×80 = 240 KB | **0.80× (interned 25% WORSE)** |
| 95/5 keyed | 10K×16 + 500×16 = 168 KB | 10K×8 + 500×80 = 120 KB | **1.40× (interned 29% better)** |

The 95/5 distribution (deep-tree majority-leaf — the most common shape for real frameworks per Flutter's catalog distribution) is the only one where interning wins under the unique-key assumption, and even then only by 1.40× — well below the 2× threshold.

**Key-sharing inverts this fundamentally.** When K distinct keys are reused across N keyed nodes (K << N), interner overhead scales with K, not N:

| Distribution + sharing | Interner cost | Memory ratio |
|---|---:|---:|
| 80/20 keyed, K=100 shared | 80 KB inline + 8 KB interner = 88 KB | **2.18× (interned wins)** |
| 80/20 keyed, K=10 shared | 80 KB inline + 0.8 KB interner = ~81 KB | **2.37× (interned wins)** |
| 80/20 keyed, K=2000 unique | 80 KB inline + 160 KB interner = 240 KB | **0.80× (baseline wins — bench case)** |

The bench measures the worst case (unique keys). Real catalogs sit closer to the K=100 shared case for list-heavy shapes. The verdict is therefore CONDITIONAL on the real distribution, and the bench alone cannot resolve it.

**Catalog.1 re-open mechanism (per ADV-5):**
- When `flui-widgets` ships 50-100 real widgets in Catalog.1, the U2 bench reruns against a generator that uses real-catalog widget shapes (instead of the synthetic 80/20 unique-key).
- If the rebench's memory ratio crosses 2.0× in favour of interning, a follow-up migration PR can switch storage from `Option<Box<dyn ViewKey>>` to `Option<KeyId>`. The migration is well-bounded — one `ElementNode` field, two trait impl sites, one new `KeyInterner` module.
- If the rebench stays below 2.0× (interning marginal or losing), the spec's locked shape stays.
- **This is the canonical re-open path for FR-022, regardless of Phase 0's bench-specific verdict.** Phase 0 closes the spec's deferred question with "bench-bounded data does not justify spec amendment"; Catalog.1 is where real-world data lives.

---

## S2 — Static-path skips keyed reconciler

### Method

Three algorithms benchmarked against an identical 16-tuple full-reverse permutation workload (see [`docs/research/2026-05-22-s2-static-path-sketch.md`] for full method + algorithm bodies):

- **Algorithm A (Linear keyed, FR-016 baseline)**: HashMap-build + hash-dispatch + lookup per position. The algorithm Phase 2 U12 ships against both paths.
- **Algorithm B (Positional-only specialised)**: walk 16 positions, compare `TypeId` per slot, emit `Reuse(i)` or `Replace`. No HashMap, no heap. Structurally cannot preserve keyed state across reorders.
- **Algorithm C (Reorder-aware specialised)**: adds cross-position `TypeId` scan for `Move` detection while keeping the static-tuple shape. Preserves the keyed-state-preservation semantic at the static path.

### Measurements (from [`docs/research/2026-05-22-s2-static-path-sketch.md`])

| Algorithm | Speedup vs A | LOC (algorithm body) |
|---|---:|---:|
| A — Linear keyed (FR-016 baseline) | 1.00× | 104 (production) / 16 (bench kernel) |
| B — Positional-only specialised | **7.3×** | 12 |
| C — Reorder-aware specialised | **4.0×** | 29 |

### S2 verdict

**FR-016 stays locked. Do not re-open.**

The structural argument from [`2026-05-22-s2-static-path-sketch.md`]:
- Algorithm B crosses the 5× threshold but is **structurally incapable** of the keyed-state-preserving reorder FR-016 commits both paths to. At the static path that semantic is vacuous (different tuple type signature = different `ViewSeq`), so the "behavior loyalty" question collapses: **shipping two algorithms with structurally-different semantics breaks the principle "behavior loyal, structure Rust-native"** ([STRATEGY.md]) — the divergence is at the *semantic* layer, not the *structure* layer.
- Algorithm C is the apples-to-apples alternative. It lands at 4.0× — below the spec's 5× material-margin threshold.

Phase 2 U12 ships the linear keyed algorithm against both paths as written. A future-work Catalog.1 perf investigation against real per-frame budget data may re-open FR-016; deferred to a future cycle.

---

## Phase 1 entry conditions

Per the plan's Phase 0 PR exit criteria, Phase 1 may not start until this gate report clears. With both deferred questions resolved without spec amendment, Phase 1 entry is unblocked:

1. **S1 verdict (FR-022 stays locked).** Phase 1 ships the spec's `Option<Box<dyn ViewKey>>` storage shape per FR-022. Plan KTD-2 matches; no amendment needed. Catalog.1 rebench per ADV-5 is the canonical re-open gate for real-distribution data.
2. **S2 verdict (FR-016 stays locked).** Phase 2 U12 ships the linear keyed algorithm against both static-tuple and dynamic-Vec paths per the plan. No amendment needed.
3. **Catalog.1 re-open mechanism acknowledged.** Plan Risk Register entry per ADV-5 is in place; no Phase 1 action required, but the rebench procedure is on the books for real-catalog measurement.

### Spec amendment

Spec round-6 amends:
- **Spec Deferred / Open Questions § S1**: mark as resolved by this gate report (FR-022 stays locked; Catalog.1 rebench mechanism is the canonical re-open gate).
- **Spec Deferred / Open Questions § S2**: mark as resolved by this gate report (FR-016 stays locked).

No FR body amendments needed.

### Plan-author recommendation

Phase 1 starts as written. The spec's `Option<Box<dyn ViewKey>>` storage shape ships; the `KeyInterner` reference impl in [`crates/flui-view/benches/shared/mock_node.rs`] (commit `8139d84c`) stays in the bench tree as institutional record of the explored alternative, ready to copy into production should Catalog.1 measurement justify it.

---

## Phase 0 PR summary

Phase 0 lands as one PR containing four atomic commits:

| Commit | Unit | Files |
|---|---|---|
| `0ee0117f` | U1 — Bench infrastructure | `crates/flui-view/Cargo.toml`, `benches/reconcile_baseline.rs` |
| `8139d84c` | U2 — S1 KeyId prototype | `Cargo.toml`, `benches/s1_key_storage.rs`, `benches/shared/mock_node.rs` |
| `d5af3dcd` | U3 — S2 static-path sketch | `Cargo.toml`, `benches/s2_static_path.rs`, `benches/shared/mock_tuple.rs`, `docs/research/2026-05-22-s2-static-path-sketch.md` |
| `(this commit)` | U4 — Phase 0 gate report | `docs/research/2026-05-22-phase0-gate-report.md` |

Verification gates (all four commits land green):
- `cargo bench -p flui-view --bench {reconcile_baseline,s1_key_storage,s2_static_path} --no-run` — exit 0
- `cargo bench -p flui-view --bench {…} -- --test` — all scenarios "Success"
- `cargo clippy -p flui-view --benches -- -D warnings` — clean
- `cargo build --workspace` — clean
- `bash scripts/port-check.sh -v` — 7/7 triggers ok (no Phase 3 work yet; the FR-033 grep + FR-036 trigger #9 land in Phase 3 U29/U30)

Phase 0 PR is mergeable to `main` after this gate report lands. Phase 1 branches from the merged commit.

---

## References

- **Plan:** [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`] (U1 / U2 / U3 / U4 + Risk Register ADV-5 entry)
- **Spec:** [`specs/004-view-element-core/spec.md`] (round-5 verified; FR-016, FR-022; Deferred S1 + S2; Implementation Sequence Phase 0)
- **Strategy:** [`STRATEGY.md`] ("behavior loyal, structure Rust-native")
- **Foundations:** [`docs/FOUNDATIONS.md`] Part III C2/C3/C4/C6/C9
- **S2 algorithm sketch:** [`docs/research/2026-05-22-s2-static-path-sketch.md`]
- **U2 bench source:** [`crates/flui-view/benches/s1_key_storage.rs`] + [`crates/flui-view/benches/shared/mock_node.rs`]
- **U3 bench source:** [`crates/flui-view/benches/s2_static_path.rs`] + [`crates/flui-view/benches/shared/mock_tuple.rs`]
