# S2 — Static-path tuple-permutation algorithm sketch

**Plan unit:** [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`] U3
**Spec deferred question:** S2 — *Static-path skips keyed reconciler*
([`specs/004-view-element-core/spec.md`] line 362)
**Spec FR re-open candidate:** FR-016 — *"Both paths share the same algorithm"*
**Bench source:** [`crates/flui-view/benches/s2_static_path.rs`] + mocks at
[`crates/flui-view/benches/shared/mock_tuple.rs`]
**Phase:** 0 (spec-validation; gates Phase 2 reconciler shape)
**Author role:** Systems Designer (Phase 0 bench unit U3)
**Date:** 2026-05-22

---

## Executive verdict

**FR-016 stays locked. Do not re-open.**

The static-path-pure specialised algorithm beats the FR-016 linear-keyed
algorithm by **~7.3×** on the 16-tuple full-reverse permutation, which crosses
the spec's 5× material-margin threshold. However — the algorithm that crosses
the threshold (**positional-only specialised**) is **structurally incapable**
of the keyed-state-preserving reorder that FR-016 commits both paths to. At
the static path that semantic is vacuous (different tuple type signature =
different `ViewSeq`), so the "behavior loyalty" question collapses: FR-016's
"both paths share the same algorithm" is a Flutter-loyalty constraint, not a
Rust-necessary one, but **shipping two algorithms with structurally-different
semantics breaks the principle "behavior loyal, structure Rust-native"**
([STRATEGY.md]) — the divergence is at the *semantic* layer, not the
*structure* layer.

The apples-to-apples alternative (**reorder-aware specialised**) lands at
~4.0× faster, which falls below the 5× threshold. Under the spec's own
material-margin gate, FR-016 holds.

**Phase 2 U12 lands the linear keyed algorithm against both paths as
written.**

A follow-up performance investigation against Catalog.1 widget timings can
re-open FR-016 with real per-frame budget data; deferred to a future cycle.

---

## The static-path observation

In a true static-tuple `(A, B, C, ..., P)` setting, the `ViewSeq`'s shape is
encoded in the tuple's type signature. "Reordering" at this path means a
literally different generic type — `(C, A, B)` is not a permutation of
`(A, B, C)` at the type level. The framework's reconciler has two honest
positions it could take:

1. **"Different tuple type = different `ViewSeq` = full positional rebuild."**
   At the static path this is correct. Keyed-state-preserving reorder is a
   `Vec<BoxedView>` concern, not a tuple concern. The algorithm reduces to:
   walk 16 positions, compare `TypeId` per slot, emit `Reuse(i)` or
   `Replace`. **No HashMap, no cross-position lookup, no heap allocation.**

2. **"Apply the linear keyed algorithm regardless."** This is what FR-016
   commits to. Costs a HashMap allocation per frame, hash dispatch per
   position, lookup per position. At the 16-tuple grain the algorithm is
   over-engineered: there are no cross-position keyed-state-preservation
   semantics to preserve.

The S2 question is: given that the linear keyed algorithm is structurally
over-engineered for the static path, does a specialised algorithm produce
**meaningfully better perf** at the 16-position grain?

---

## Algorithms compared

Three algorithms run against an identical 16-tuple full-reverse permutation
input. All three return `[ReconcileAction; 16]`, stack-allocated; the bench
measures algorithm-shape cost in isolation, not divergent return-type
construction cost.

### Algorithm A — Linear keyed (FR-016 baseline)

```rust
// O(N) average. HashMap allocation + hash dispatch per position.
pub fn reconcile_linear_keyed(
    old: &[TypeIdSlot; 16],
    new: &[TypeIdSlot; 16],
) -> [ReconcileAction; 16] {
    let mut keyed: HashMap<u64, u8> = HashMap::with_capacity(16);
    for (i, slot) in old.iter().enumerate() {
        keyed.insert(slot.key_hash, i as u8);
    }
    let mut out = [ReconcileAction::Replace; 16];
    for (i, slot) in new.iter().enumerate() {
        if let Some(&old_idx) = keyed.get(&slot.key_hash) {
            out[i] = ReconcileAction::Reuse(old_idx);
        }
    }
    out
}
```

**Shape match against production reconciler.** The kernel shape is the same
as the keyed-map-build + walk phases at
[`crates/flui-view/src/tree/reconciliation.rs:91-177`]. Bench-kernel LOC is
~16 (excluding doc-comment); production-reconciler full body is 143 LOC
(prefix/suffix scan + the keyed middle stub at lines 91-98 + Phase 4 walk +
Phase 5 removal). The bench-kernel measures the keyed-middle dominant cost.

### Algorithm B — Positional-only specialised (pure static-path shape)

```rust
// O(N). Stack-only. No HashMap, no cross-position lookup.
pub fn reconcile_positional_specialised(
    old: &[TypeIdSlot; 16],
    new: &[TypeIdSlot; 16],
) -> [ReconcileAction; 16] {
    let mut out = [ReconcileAction::Replace; 16];
    for i in 0..16 {
        if old[i].type_id == new[i].type_id {
            out[i] = ReconcileAction::Reuse(i as u8);
        }
    }
    out
}
```

**The shape a real `const fn reconcile_tuple_16<A, B, ..., P>(...)` would
compile to.** The per-position TypeId comparison is the only operation. At
the limit, with monomorphisation, this could devirtualise into a 16-way
chain of `TypeId::of::<An>() == TypeId::of::<Bn>()` constant comparisons —
each one either folds to `true` (positional match) or `false` (positional
replace) at compile time. The bench cannot exercise this limit because the
mock's `TypeId` is a runtime value; the doc verdict addresses both the
measured runtime cost and the theoretical compile-time-fold ceiling.

**Cannot do cross-position reorder.** If the tuple type signature changes,
every position reports `Replace`. **At the static path this is the correct
answer** — a reversed tuple is a different `ViewSeq` type, and the
framework's positional reconciler is the right answer.

### Algorithm C — Reorder-aware specialised

```rust
// O(N). Stack-allocated [Option<u8>; 16] index. No heap.
pub fn reconcile_reorder_specialised(
    old: &[TypeIdSlot; 16],
    new: &[TypeIdSlot; 16],
) -> [ReconcileAction; 16] {
    let mut index: [Option<u8>; 16] = [None; 16];
    for (i, slot) in old.iter().enumerate() {
        let mut bucket = (slot.key_hash as usize) % 16;
        loop {
            if index[bucket].is_none() {
                index[bucket] = Some(i as u8);
                break;
            }
            bucket = (bucket + 1) % 16;
        }
    }
    let mut out = [ReconcileAction::Replace; 16];
    for (i, slot) in new.iter().enumerate() {
        let mut bucket = (slot.key_hash as usize) % 16;
        loop {
            match index[bucket] {
                Some(old_idx) if old[old_idx as usize].key_hash == slot.key_hash => {
                    out[i] = ReconcileAction::Reuse(old_idx);
                    break;
                }
                Some(_) => bucket = (bucket + 1) % 16,
                None => break,
            }
        }
    }
    out
}
```

**Apples-to-apples comparison against the linear keyed algorithm.** Same
O(N) big-O. Same cross-position-reorder capability. Differs only in
allocation strategy: stack-allocated `[Option<u8>; 16]` instead of
`HashMap<u64, u8>`. This is the algorithm that preserves FR-016's
keyed-state-preservation semantic at the static path.

---

## Results — `cargo bench -p flui-view --bench s2_static_path -- --quick`

| Algorithm | Time (ns/iter, mid) | Speedup vs FR-016 baseline | Crosses 5× threshold? |
|---|---|---|---|
| **A — Linear keyed (FR-016 baseline)** | 277.4 ns | 1.0× | — |
| **B — Positional-only specialised** | 38.3 ns | **7.3×** | **yes** |
| **C — Reorder-aware specialised** | 69.7 ns | 4.0× | no |

Bench machine: Windows 11, criterion 0.7, `--quick` mode (1-shot
measurement; 3-sigma confidence interval visible in the `cargo bench` output
captured in U3's commit body).

**Stability caveat.** Numbers are `--quick`-mode point estimates, not full
criterion 100-sample regimes. The full bench is reproducible via
`cargo bench -p flui-view --bench s2_static_path`; ordering across the three
algorithms is robust to that switch (sigmas don't overlap), but absolute
nanosecond figures may drift ±10% across machines.

---

## Complexity / LOC tabulation

| Algorithm | Big-O | Heap allocation per frame | LOC (algorithm body) | Cyclomatic complexity |
|---|---|---|---|---|
| **A — Linear keyed (bench kernel)** | O(N) | 1× `HashMap::with_capacity(16)` | 16 | 4 |
| **A — Linear keyed (production at `reconciliation.rs:51-193`)** | O(N) | 1× HashMap + 1× `Vec<bool>` + 1× `Vec<(ElementId, usize)>` | 104 | 18 |
| **B — Positional-only specialised** | O(N), trivial | none | 12 | 3 |
| **C — Reorder-aware specialised** | O(N) avg, O(N²) worst | none (stack-only `[Option<u8>; 16]`) | 29 | 6 |

**Cyclomatic complexity** computed by hand: count of `if` / `else` / `match`
arms / `while` / `for` / `&&` / `||` branch points + 1. The production-Phase-2
`reconcile_children` is the keyed-middle-plus-scaffold shape (prefix scan +
suffix scan + keyed map build + middle walk + Phase 5 remove), so its LOC
and CC are higher than the bench kernel by design — the bench kernel
isolates the keyed-middle dominant cost.

**LOC asymmetry note.** Algorithm B is 12 LOC, less than half of Algorithm C
at 29 LOC. The asymmetry is the cross-position-reorder code — Algorithm B
omits it entirely (because at the static path it is structurally vacuous);
Algorithm C carries it (because it is the apples-to-apples comparison
against Algorithm A). The 12-vs-29 LOC delta IS the "different semantic"
delta the verdict turns on.

---

## Parity judgment per FR-016 + STRATEGY.md "behavior loyal"

FR-016 commits both paths to "the same algorithm" — the keyed reconciler
ported from Flutter's `RenderObjectElement.updateChildren`. The S2 question
is whether a static-path-specialised algorithm preserves that contract.

**Three claims to test:**

1. **Does the specialised algorithm preserve Flutter's
   `RenderObjectElement.updateChildren` *behavior* at the static path?**

   - **Algorithm B (positional-only)**: NO. Flutter's algorithm is defined
     on `List<Widget>` (dynamic, ordered, key-preserving). It has no tuple
     equivalent — `Iterable<Widget>` in Dart erases positional type identity
     at runtime. At the static path with a *different* tuple type, Algorithm
     B reports every position as `Replace`. The keyed-state-preserving
     reorder semantic Flutter's algorithm provides is **structurally absent
     from Algorithm B** — not just a different implementation; a different
     contract. **DIVERGES.**

   - **Algorithm C (reorder-aware)**: YES. Same O(N) shape, same outputs
     for any 16 distinct TypeIds, same keyed-state-preserving semantic.
     Differs from Algorithm A only in allocation strategy (stack vs heap).
     **PRESERVES.**

2. **Does the spec's 5× material-margin threshold pass?**

   - Algorithm B at **7.3×** crosses the threshold.
   - Algorithm C at **4.0×** falls below.

3. **Does FR-016's "both paths share the same algorithm" constraint survive
   if we adopt the specialised algorithm?**

   - **Algorithm B**: breaks the constraint at the semantic layer (the
     static path no longer preserves keyed-state across hypothetical
     reordering, even though that reordering is structurally vacuous at
     the static path). The constraint can be **reinterpreted** as "the
     dynamic path implements Flutter's algorithm; the static path applies
     the algorithm trivially because reordering is structurally
     vacuous" — but this is a re-statement of the constraint, not a
     preservation.

   - **Algorithm C**: preserves the constraint but does not cross the
     material-margin gate.

**Verdict on FR-016 re-open candidacy:**

- If the bar is **"specialised algorithm beats by 5× AND preserves
  behavior"**: **FR-016 stays locked. No re-open.** No algorithm hits both
  bars simultaneously.

- If the bar is **"specialised algorithm beats by 5× ignoring behavior
  divergence"**: Algorithm B crosses, FR-016 could re-open in favor of a
  static-path-specialised algorithm. **But this would commit FLUI to a
  two-algorithm framework where the static path and the dynamic path have
  structurally-different keyed semantics — a divergence at the semantic
  layer, not the structure layer. This violates STRATEGY.md "behavior
  loyal, structure Rust-native" — algorithms come 1:1 from Flutter;
  Rust-native means closed enums + split-borrow + tracing, NOT a different
  algorithm.**

The "behavior loyal" principle decides this: **FR-016 stays locked**. The
static-path-specialised algorithm's win is real but accidental — it wins
because the static path has fewer semantic obligations, not because the
algorithm is structurally cleaner. Shipping the win would commit FLUI to
explaining "keyed-state-preserving reorder works on `Vec<BoxedView>` but
not on tuples" to every future widget author — a silent footgun analogous
to the FR-022 storage-shape silent-correctness traps that prompted this
spec in the first place.

---

## What re-opening FR-016 would require (out-of-scope, documented for U4)

A future cycle could revisit FR-016 under one of two regimes:

1. **Catalog.1 perf reality check.** If Catalog.1 (the widget catalog
   business unit) measures that the FR-016 linear keyed algorithm
   dominates per-frame budget at typical real widget counts (15-30
   children per `Column`), and the 7.3× speedup at static-tuple grain
   would shift the budget meaningfully, the verdict above can be
   re-examined with real per-frame budget data. The current 277-ns vs
   38-ns delta is 239 ns per `Column` at the 16-tuple worst case — a 60fps
   frame is 16.67 ms / ~16.67 million ns, so the delta is ~0.0014% of
   frame budget per `Column`. Below the noise floor of every other
   per-frame cost.

2. **Constitution amendment for two-algorithm architecture.** Spec
   re-write to articulate "static path and dynamic path have
   structurally-different keyed semantics; tuple children are
   positionally-keyed only, and `Vec<BoxedView>` children carry full
   keyed-state-preserving reorder". The amendment would need to pass the
   "no silent correctness footgun" bar (SC-002's keyed-state-preservation
   corpus would not apply to the static path; this would need to be
   prominent in every multi-child widget's documentation).

Neither regime is in scope for this plan unit. **U4's gate report records
FR-016 as locked.**

---

## Reproducing the bench

```bash
# Compile only
cargo bench -p flui-view --bench s2_static_path --no-run

# Run all three benches (full criterion regime, ~30 seconds)
cargo bench -p flui-view --bench s2_static_path

# One-shot --quick mode (the numbers in this doc)
cargo bench -p flui-view --bench s2_static_path -- --quick

# Just verify benches run (test-mode, 1 iter each)
cargo bench -p flui-view --bench s2_static_path -- --test
```

Bench outputs land under `target/criterion/s2_static_path/`. Full HTML
reports include statistical histograms and outlier counts.

---

## Files in this unit

- [`crates/flui-view/benches/s2_static_path.rs`] — criterion bench
- [`crates/flui-view/benches/shared/mock_tuple.rs`] — 16-position mocks +
  three algorithm implementations
- [`crates/flui-view/Cargo.toml`] — `[[bench]] name = "s2_static_path"`
  declaration
- [`docs/research/2026-05-22-s2-static-path-sketch.md`] — this doc

---

## One-line verdict (for U4 gate report and downstream forwarding)

**FR-016 stays locked. Specialised algorithm beats by 7.3× but breaks
behavior loyalty; apples-to-apples preserving variant is only 4.0× and falls
below the 5× threshold. STRATEGY.md "behavior loyal" decides this.**

[`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`]: ../plans/2026-05-22-005-feat-view-element-core-contracts-plan.md
[`specs/004-view-element-core/spec.md`]: ../../specs/004-view-element-core/spec.md
[`crates/flui-view/benches/s2_static_path.rs`]: ../../crates/flui-view/benches/s2_static_path.rs
[`crates/flui-view/benches/shared/mock_tuple.rs`]: ../../crates/flui-view/benches/shared/mock_tuple.rs
[`crates/flui-view/Cargo.toml`]: ../../crates/flui-view/Cargo.toml
[`crates/flui-view/src/tree/reconciliation.rs:51-193`]: ../../crates/flui-view/src/tree/reconciliation.rs
[`crates/flui-view/src/tree/reconciliation.rs:91-177`]: ../../crates/flui-view/src/tree/reconciliation.rs
[`docs/research/2026-05-22-s2-static-path-sketch.md`]: ./2026-05-22-s2-static-path-sketch.md
[STRATEGY.md]: ../../STRATEGY.md
