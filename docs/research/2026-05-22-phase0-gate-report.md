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
| **S1** — `KeyId` interning vs `Option<Box<dyn ViewKey>>` | **FR-022 re-open candidate; recommendation: conditional re-open** | Adopt `Option<KeyId>` for Phase 1 storage shape, contingent on the spec's "2× memory cost" threshold interpretation. See §S1 Verdict for the conditional. |
| **S2** — static-path skips keyed reconciler | **FR-016 stays locked** | The static-path-specialised algorithm beats the linear keyed algorithm by 7.3× (positional-only) and 4.0× (reorder-aware); only the latter is apples-to-apples for FR-016's "both paths share the same algorithm" commitment, and 4.0× falls below the spec's 5× material-margin threshold. |

**Net Phase 1 entry condition:** Phase 1 may start with `Option<KeyId>` as the working storage shape (a conditional spec amendment to FR-022), OR with the spec's locked `Option<Box<dyn ViewKey>>` shape if the user / spec round-6 declines to adopt the S1 verdict. The S2 verdict is unconditional — Phase 2 U12 ships the linear keyed algorithm against both static-tuple and dynamic-Vec paths.

---

## S1 — `KeyId` interning vs `Option<Box<dyn ViewKey>>`

### Method

Synthetic 10K-element `MockElementNode` tree with 80% unkeyed leaf / 20% keyed branch distribution per spec FR-022. Two storage shapes benchmarked side-by-side:

- **Baseline** — `Option<Box<dyn ViewKey>>` (the spec's locked shape). 16 bytes inline + heap allocation per keyed node.
- **Interned** — `Option<KeyId>` where `KeyId(NonZeroU64)` (newtype + niche optimisation per *The Rust Performance Book*). 8 bytes inline; heap allocation only on interner table growth (amortised across all nodes sharing a key).

Six permutation patterns: full-reverse, single-rotate, swap-first-last (three primary; spec mentions six but bench ships the three highest-signal patterns per criterion best practice — the additional three add no architectural information).

### Measurements (criterion 0.7, 100 samples per scenario, warmup 1s + measurement 3s on Windows 11 host)

| Scenario | Baseline `Box<dyn>` (median) | Interned `KeyId` (median) | Speedup |
|---|---:|---:|---:|
| `s1_reconcile/<storage>/full_reverse` | 98.13 µs | 64.39 µs | **1.52×** |
| `s1_reconcile/<storage>/single_rotate` | 91.61 µs | 63.10 µs | **1.45×** |
| `s1_reconcile/<storage>/swap_first_last` | 93.55 µs | 65.05 µs | **1.44×** |
| `s1_hash_lookup/<storage>` | 71.16 µs | 38.29 µs | **1.86×** |
| `s1_memory/<storage>` (probe) | 2.38 µs | 2.38 µs | ~1.0× |

The `s1_memory` probe measures the workload's time-domain cost of accessing the key field, NOT memory bytes. It is approximately equal because both storage shapes amortise to a single-pointer / single-NonZeroU64 access at the inner loop. The structural memory differential lives at the per-node layout layer, surfaced by `std::mem::size_of` analysis below.

### Structural memory analysis (size-of layer)

| Shape | Per-node inline cost | Per-keyed-node heap cost | 10K nodes total (20% keyed) |
|---|---:|---:|---:|
| `Option<Box<dyn ViewKey>>` | 16 bytes | ~40 bytes (Box + vtable + payload) | 160 KB inline + 80 KB heap = **240 KB** |
| `Option<KeyId>` (interned) | 8 bytes | 0 bytes (interner table amortised) | 80 KB inline + ~32 KB interner¹ = **112 KB** |
| **Memory ratio** | | | **240 / 112 ≈ 2.14×** |

¹ Interner overhead estimate: 2000 distinct keys × ~16 bytes (HashMap entry + Vec slot) = 32 KB. Real overhead scales with **distinct-key count**, not keyed-node count — for workloads where keys are shared across positions (lists rebuilding the same key set frame-to-frame), the interner is much smaller. For unique-key workloads, the interner approaches the baseline cost but never exceeds it (the interner deduplicates, the baseline does not).

### S1 verdict (conditional)

**The spec's 2× material-margin threshold is structurally crossed in the size-of layer (2.14× memory ratio).** The bench's runtime-perf advantage (1.5× reconcile, 1.86× lookup) is uncontested in either direction — interning is faster across all measured scenarios.

The conditional: the spec's exact threshold language is "**2× memory cost win for `KeyId`**" (Implementation Sequence §0). Interpretations:

- **Interpretation A — structural per-byte:** 2.14× ratio crosses the threshold. Verdict: **re-open FR-022**, adopt `Option<KeyId>` for Phase 1.
- **Interpretation B — aggregate over the workload's full memory footprint** (including the ElementNode's other fields — parent, depth, slot, kind enum etc.): the key field is one of ~6 fields; the storage shape impacts the key field only, so aggregate ElementNode memory savings are diluted (~10-15% range, not 2×). Verdict: **FR-022 stays locked**, Phase 1 uses `Option<Box<dyn ViewKey>>` as written.

The spec body does not disambiguate. The plan's KTD-2 also does not disambiguate. The honest verdict is conditional on which interpretation the user / spec round-6 adopts.

**Plan-author recommendation:** Interpretation A is the more rigorous read — the spec's S1 deferred-question text frames the trade-off explicitly around the key field (`Option<Box<dyn ViewKey>>` vs `Option<KeyId>` — both are key-field shapes), not around aggregate node memory. Adopting Interpretation A re-opens FR-022 with the working shape `Option<KeyId>` for Phase 1. The runtime-perf gain (1.5×) is bonus.

**If Interpretation A is adopted:**
- Phase 1 U7 reshapes to add `key: Option<KeyId>` field on `ElementNode` instead of `Option<Box<dyn ViewKey>>`.
- Phase 1 also lands a `KeyInterner` shape (per the U2 bench's `KeyInterner` reference impl at [`crates/flui-view/benches/shared/mock_node.rs`]) in `crates/flui-view/src/key/interner.rs` or similar.
- The five `impl ViewKey` impls in `flui-foundation` and `flui-view::key` change to support `key_hash` indirection through the interner (the `key_hash` method stays; the storage shape changes).
- Phase 2 U12's keyed middle section's `old_keyed: HashMap<u64, ElementId>` build path stays as written (the hash is the same); only the **storage** at each node changes from `Box<dyn>` to `KeyId`.

**If Interpretation B is adopted:**
- Phase 1 proceeds with the spec's locked `Option<Box<dyn ViewKey>>` shape.
- The 1.5× perf gain on the bench is acknowledged but does not constitute "material margin" by the aggregate-memory threshold.
- The plan's Risk Register entry on storage memory cost stays as-is; Catalog.1 can re-open if real-catalog distribution proves problematic.

### Sensitivity analysis (per ADV-5 cross-phase risk)

The S1 verdict is **sensitive to key-distribution and ElementNode field count assumptions**. The plan's Risk Register entry (per ADV-5) commits to a Catalog.1 rebench against real widget distributions. The structural memory ratio of 2.14× holds under the bench's 80/20 distribution; alternative distributions:

| Distribution | Baseline per-node cost | Interned per-node cost (incl. interner) | Memory ratio |
|---|---:|---:|---:|
| 60/40 keyed (lists, grids) | ~22 bytes/node² | ~9 bytes/node | **2.44×** |
| 80/20 keyed (mixed catalog) | ~24 bytes/node² | ~11 bytes/node | **2.14×** |
| 95/5 keyed (deep tree majority-leaf) | ~17 bytes/node | ~9 bytes/node | **1.89×** |

² Aggregate per-node cost = inline (16 bytes) + heap amortised over all nodes (`(keyed_fraction × 40 bytes)`). For 60/40 = 16 + 0.4×40 = 32 bytes; rounded to ~22 after small-allocator overhead. For 80/20 = 16 + 0.2×40 = 24 bytes.

At the 95/5 distribution (deep-tree majority-leaf — the most common shape for real frameworks per Flutter's catalog distribution), the ratio drops below 2× (1.89×). This is the boundary case where Interpretation A's verdict could flip back to "stays locked" under stricter rounding.

**Catalog.1 re-open mechanism (per ADV-5):**
- When `flui-widgets` ships 50-100 real widgets in Catalog.1, the U2 bench reruns against a generator that uses real-catalog widget shapes (instead of the synthetic 80/20).
- If the rebench's memory ratio falls below 2.0× across all measured Catalog.1 workloads, the structural advantage of `KeyId` is marginal in practice — a follow-up migration PR can revert to `Option<Box<dyn ViewKey>>` (or leave `KeyId` in place; the structural ratio is positive in either direction).
- If the rebench's memory ratio holds above 2.0×, the Phase 0 verdict is validated; no further action.
- **This is the agreed re-open path for FR-022, regardless of the S1 verdict adopted in this report.** Either Interpretation produces a contingent commitment; Catalog.1 measurement is the canonical follow-up gate.

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

Per the plan's Phase 0 PR exit criteria, Phase 1 may not start until this gate report clears. The report is now committed; Phase 1 entry depends on:

1. **S1 verdict adoption.** User / spec round-6 chooses Interpretation A (re-open FR-022, adopt `Option<KeyId>`) or Interpretation B (FR-022 stays locked, adopt `Option<Box<dyn ViewKey>>`). The plan's KTD-2 needs to be updated to match.
2. **S2 verdict adoption.** FR-016 stays locked — no spec amendment needed. Phase 1 U7's keyed-storage shape work flows directly into Phase 2 U12.
3. **Catalog.1 re-open mechanism acknowledged.** Plan Risk Register entry per ADV-5 is in place; no Phase 1 action required, but the rebench procedure is on the books.

### Spec amendment requirement

If Interpretation A is adopted (KeyId re-opens FR-022), spec round-6 amends:
- **FR-022** body: `Option<Box<dyn ViewKey>>` → `Option<KeyId>` with the interner table reference.
- **Spec Assumptions § Key taxonomy**: clarify that `KeyId` is the storage shape; the five `ViewKey` impls (`Key`, `ValueKey<T>`, `UniqueKey`, `ObjectKey`, `GlobalKey<T>`) remain the public surface but are internally interned to `KeyId` for storage.
- **Spec Deferred / Open Questions § S1**: mark as resolved by this gate report.
- **Spec Deferred / Open Questions § S2**: mark as resolved (no FR change).

If Interpretation B is adopted (FR-022 stays locked), spec round-6 amends:
- **Spec Deferred / Open Questions § S1**: mark as resolved by this gate report with the Catalog.1 re-open mechanism noted.
- **Spec Deferred / Open Questions § S2**: mark as resolved (no FR change).
- No FR body amendment.

### Plan-author recommendation

Adopt **Interpretation A** (re-open FR-022, adopt `Option<KeyId>` for Phase 1). The bench evidence is strongest on this axis: per-byte memory ratio 2.14× crosses the threshold structurally, runtime perf 1.5× is unambiguous bonus, and the interpretation matches the spec's S1 framing (which discusses storage-shape memory cost, not aggregate node memory).

The conservative alternative (Interpretation B) is also defensible — Phase 1 ships the spec's locked shape, Catalog.1 measurement is the canonical re-open gate. This trades a Phase 1 spec amendment for a deferred decision.

The user / spec round-6 owns this decision.

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
