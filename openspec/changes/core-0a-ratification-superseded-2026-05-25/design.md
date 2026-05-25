# Design — `core-0a-foundation-parity-to-flutter`

| Field | Value |
|---|---|
| Change ID | `core-0a-foundation-parity-to-flutter` |
| Phase | sdd-plan / design step |
| Owner crates | `crates/flui-foundation`, `crates/flui-tree` |
| Source spec set | 8 domain specs under `openspec/changes/.../specs/` (74 requirements, 138.5 KB) |
| Source proposal | `openspec/changes/.../proposal.md` (re-scoped — see proposal §0) |
| Source audit | `docs/research/2026-05-22-flui-foundation-tree-audit.md` (47 findings; 34 closed by cycle 3, 13 deferred) |
| Tradeoffs required | yes (`openspec/config.yaml rules.design.require_tradeoffs: true`) |
| Strict TDD | yes (`rules.apply.strict_tdd: true`) |
| Review budget per task | 400 changed lines |

> **Read first.** This design is written against the **post-cycle-3 reality** documented in the proposal §0 scope-drift notice. Cycle 3 (PRs #102–#106 + Polish PR, completed 2026-05-22 → 2026-05-23) already shipped the task-brief's headline keystones — `TreeWrite::remove` cascade-by-default trait contract (T-1), depth-constant consolidation (T-10), `BindingBase` init-after-panic fix (I-3), `ObserverList` deletion (I-1), `flui-log` merge, and `port-check.sh` triggers #8–#13 (PR #151). **34 of 47 audit findings closed; 13 explicitly deferred for design adjudication.** This design's deliverable is therefore the *ratification* artifact for those shipped decisions plus the *verdict* artifact for the 13 deferrals. Tradeoffs are recorded so the next audit cycle does not re-litigate.

---

## 1. Executive summary

The 9 design decisions the supervisor's task brief enumerated are addressed as follows:

| # | Brief's decision | Status going into design | Design outcome |
|---|---|---|---|
| 1 | `TreeWrite::remove` cascade-default + opt-out | Landed cycle 3 PR #103 Wave 1+2 | **Ratify** as permanent trait contract; record tradeoffs (§4.1) |
| 2 | Depth constant consolidation | Landed cycle 3 PR #104 Wave 3 | **Ratify** `MAX_TREE_DEPTH=256` + `INLINE_TREE_DEPTH=32` as **two-constant** (not derived sub-constant) shape; record tradeoffs (§4.2) |
| 3 | Zombie surface disposition per module | Landed cycle 3 PRs #103/#105 (delete chosen over feature-gate) | **Ratify** per-module table (§4.3) |
| 4 | `BindingBase` init-after-panic ordering | Landed cycle 3 PR #103 | **Ratify** `OnceLock::get_or_init` → `AtomicBool::store(Release)`-after-success ordering (§4.4) |
| 5 | `ObserverList<T>` decision | Landed cycle 3 PR #103 (deleted) | **Ratify** delete with evidence (§4.5) |
| 6 | Migration ordering across PRs | n/a until this change | **NEW** — propose 3 doc PRs + optional 2 contingent code PRs (§5) |
| 7 | Breaking change inventory | n/a until this change | **NEW** — empty default path; 1-line PORT.md edit; ≤3 additive code changes in contingent path (§6) |
| 8 | `port-check.sh` additions for refusal triggers #8–#13 | Landed PR #151 | **Confirm** installation and verify gate (§7) |
| 9 | Engram protocol for apply phase | n/a until this change | **NEW** — per-PR topic-key schema + Engram-unavailable fallback (§8) |

Plus the proposal's mandate items:

| Item | Owner spec | Status | Design outcome |
|---|---|---|---|
| Author `crates/flui-tree/ARCHITECTURE.md` | `tree-architecture-md/spec.md` | NEW for sdd-apply | §5 PR1 task |
| Amend `crates/flui-foundation/ARCHITECTURE.md` | mirrored in foundation specs | NEW for sdd-apply | §5 PR2 task |
| Flip `docs/PORT.md` Index row | `tree-architecture-md/spec.md` R10 | NEW for sdd-apply | §5 PR2 task |
| Parity-verification research doc | proposal §2.3 | NEW for sdd-apply | §5 PR3 task; row template (§9) |
| 13 deferred-finding verdicts | per-domain specs (foundation-* + tree-*) | Drafted in specs | **Ratify** in §4.6 table (canonical) |

**Net delta of this change (default path):**
- 3 new documents (~400 + ~400 + ~300 lines).
- 1 amendment to existing ARCHITECTURE.md (~80 lines added/modified).
- 1 single-line edit to `docs/PORT.md`.
- **Zero code changes** unless §4.6's verdict table flips any of I-7 / I-8 / I-21 to `revisit-now` (current verdict for all three: `accept-permanent`).
- Total review-line budget projected: ~1,200 lines across 3 PRs, all under the 400-line per-task budget.

---

## 2. Context — what cycle 3 already shipped

The 8 spec files in this change codify the **post-cycle-3 canonical contract**. Every requirement carries `**Audit ref:**` traceability. The "what landed" baseline is:

| Theme | Cycle-3 shipped artefact | Spec home |
|---|---|---|
| Cascade-by-default `TreeWrite::remove` + iterative + 2k-deep regression test | `crates/flui-tree/src/traits/write.rs:90+` | `tree-treewrite-contract/spec.md` R1, R2 |
| `TreeWrite::remove_shallow` opt-out primitive | `crates/flui-tree/src/traits/write.rs` | `tree-treewrite-contract/spec.md` R3 |
| Parallel mutation APIs in LayerTree/SemanticsTree consolidated to trait impls | `crates/flui-layer/src/tree/`, `crates/flui-semantics/src/` | `tree-treewrite-contract/spec.md` R8 |
| Single `MAX_TREE_DEPTH = 256`, `INLINE_TREE_DEPTH = 32`, `ROOT_DEPTH = 0` | `crates/flui-tree/src/depth.rs` | `tree-depth-canonical/spec.md` R1, R2, R6 |
| `Descendants::next` loop-based (no recursion); `Ancestors::next` step-cap | `crates/flui-tree/src/iter/{descendants,ancestors}.rs` | `tree-depth-canonical/spec.md` R8, R9 |
| `BindingBase` `INITIALIZED.store` after-`new()`-returns ordering + regression test | `crates/flui-foundation/src/binding.rs` | `foundation-binding/spec.md` R3 |
| `ObserverList`, `FoundationError`/`ErrorContext`, `WasmNotSend`, `assert.rs` macros, `consts.rs::approx_equal*` deleted | n/a (files absent) | `foundation-listenable-changenotifier/spec.md` R7, R8 + implicit-coverage in tasks verify |
| `ChangeNotifier::notify_listeners` `SmallVec<[CB; 4]>` snapshot-then-fire | `crates/flui-foundation/src/notifier.rs` | `foundation-listenable-changenotifier/spec.md` R3 |
| `ValueNotifier::into_value` calls `dispose()`; `ListenerCallback` explicit `+ 'static`; `ParseDiagnostic*Error` use `Box<str>` | `crates/flui-foundation/src/{notifier,callbacks}.rs` | `foundation-listenable-changenotifier/spec.md` R5, R6 |
| `Default for Key` / `Default for UniqueKey` deleted | `crates/flui-foundation/src/key.rs` | `foundation-key/spec.md` R4 |
| `TreeError::ArityViolation(#[from] ArityError)` + `TreeError::Internal(Box<str>)` | `crates/flui-tree/src/error.rs` | `tree-treewrite-contract/spec.md` R7 |
| `Identifier::From<Index>` always available (no `#[cfg(test)]`) | `crates/flui-foundation/src/id.rs` | `foundation-id-system/spec.md` R5 |
| `state.rs`, `visitor/`, `diff.rs`, four `iter/*` files, three `arity/*storage*` files, `traits/node.rs`, `MountableExt` deleted (~10.6K LOC) | n/a (files absent) | `tree-surface-reduction/spec.md` R1–R7 |
| `lowest_common_ancestor` and `Siblings::new` use `SmallVec<[I; INLINE_TREE_DEPTH]>` | `crates/flui-tree/src/iter/`, `traits/nav.rs` | `tree-depth-canonical/spec.md` R2 |
| `port-check.sh` refusal triggers #8–#13 installed | `scripts/port-check.sh:396-1005` | Verified in §7 below |
| `flui-log` merged into `flui-foundation/src/log/` | `crates/flui-foundation/src/log/`; `lib.rs:18` cite | `tree-architecture-md/spec.md` R7 amendment scenario |

**The design.md's job is not to re-decide these. It is to record the trade-off resolution that closes each in the project's permanent ledger.**

---

## 3. Methodology — why no peer-review broadcast

The task brief invites use of the `multi-agent` skill for cross-vendor peer review on "TreeWrite cascade contract" and "ObserverList decision". After reading both decisions in their landed form (`traits/write.rs` + the audit's I-1 closure), peer-review was deliberately **not** invoked because:

1. **Decisions have already shipped.** Both contracts are in `main` and consumed by every workspace adopter. Re-litigating via fresh peer review would directly contradict the proposal's §2.4 stance: "Does not re-attempt any of the 34 cycle-3-closed findings."
2. **Cycle-3 peer review already happened.** PR #103 carried Codex P2 review (caught the unbounded-recursion concern that became the 2k-deep regression test); the Mythos audit was the broad-context architecture review (47 findings, all triaged with severity).
3. **The remaining open work is documentation, not architecture.** Peer-review value is highest on hard architectural forks; this change's open work is verdict adjudication on 13 deferred items with well-documented rationale already on file.

**If §4.6's verdict table is overridden by the supervisor to flip any deferred item to `revisit-now` (code change), peer-review will be invoked at sdd-apply for that specific decision** — Codex for the Rust-ownership angle, Gemini for whole-crate context.

---

## 4. Architectural decisions

Each decision below follows the required structure: **Decision → Alternatives considered → Tradeoffs → Resolution → Tradeoff that resolved**.

### 4.1 — `TreeWrite::remove` contract reshape (RATIFY)

**Decision (landed cycle 3 PR #103 Wave 1+2; ratified here):**
`TreeWrite<I>::remove(&mut self, id: I) -> Option<Self::Node>` is **cascade-by-default with post-order semantics**. The implementation is the trait's default method body (iterative worklist on the heap, never recursive `self.remove(child_id)`). Opt-out lives in `remove_shallow(id) -> Option<Self::Node>` — the trait's required primitive that leaves descendants in the storage as orphans for re-parenting workflows. `remove_subtree(id) -> usize` is the count-returning convenience wrapper.

```rust
// crates/flui-tree/src/traits/write.rs (canonical form)
pub trait TreeWrite<I: Identifier>: TreeRead<I> {
    type Node;

    // Required primitive — adopters must implement.
    fn remove_shallow(&mut self, id: I) -> Option<Self::Node>;

    // Default cascade — adopters may override for storage-specific bulk ops,
    // but MUST preserve post-order semantics.
    fn remove(&mut self, id: I) -> Option<Self::Node> {
        let mut worklist: Vec<I> = Vec::new();
        worklist.push(id);
        // collect descendants via iterative TreeNav walk … (see write.rs:90+)
        // remove in post-order
        // return the original root node
    }

    // Convenience wrapper.
    fn remove_subtree(&mut self, id: I) -> usize
    where Self: TreeNav<I> + Sized { /* count + remove */ }
}
```

**Alternatives considered:**

| Alternative | Why rejected |
|---|---|
| **A. Keep `remove` non-cascade (pre-cycle-3 footgun)** | Audit T-1 critical: caller orphans descendants silently; the audit's "Most important finding" of cycle 3 specifically called this out. Every adopter implemented its own cascade independently with subtly different semantics. |
| **B. Cascade-by-default but recursive `self.remove(child_id)`** | PR #103 Codex P2 review caught this: recursive default crashes on linear-chain trees deeper than ~5k nodes on default Windows stacks. The 2k-deep regression test would have caught it eventually; iterative is the universal fix. |
| **C. Two separate traits (`CascadingWrite` + `ShallowWrite`)** | Doubles the bound surface every consumer threads through generic code. Trait composition in Rust does not have Dart's mixin-stack semantics — splitting adds friction without value. Single trait + two methods is the Rust-native idiom. |
| **D. Cascade as a free function `cascade_remove<T: TreeWrite>(t: &mut T, id)` outside the trait** | Loses default-impl override headroom (an arena-backed tree can't bulk-free without overriding). Forces every adopter to remember to call the free function. The trait-default shape gives override flexibility AND default safety. |

**Tradeoff resolved:** Safety-by-default (cascade) + escape-hatch primitive (`remove_shallow`) + override headroom (default impl on a trait method) collectively beat every alternative. The breaking-change cost was paid in cycle 3 (PR #103) when the only adopter was `RenderTree`; LayerTree/SemanticsTree adopted the new contract in PR #105 Wave 4+5 with zero observable downstream breakage (verified by Appendix A.10 reverse-dep grep).

**Spec home:** `tree-treewrite-contract/spec.md` R1, R2, R3, R4, R8.

**Verification at sdd-apply:** No code change; the existing implementation IS the canonical contract. The spec scenarios (`remove_cascades_by_default`, `remove_cascade_is_stack_safe_on_deep_chain`, `remove_shallow_does_not_cascade`) are existing regression tests.

---

### 4.2 — Depth constant consolidation (RATIFY)

**Decision (landed cycle 3 PR #104 Wave 3; ratified here):**
**Two** independent constants in `crates/flui-tree/src/depth.rs`:

```rust
pub const MAX_TREE_DEPTH: usize = 256;   // validation cap (Depth::new_checked rejects above)
pub const INLINE_TREE_DEPTH: usize = 32; // SmallVec inline cap (heap fallback above)
pub const ROOT_DEPTH: usize = 0;          // semantic constant
const _: () = { assert!(INLINE_TREE_DEPTH <= MAX_TREE_DEPTH); };
```

`INLINE_TREE_DEPTH` is **NOT** derived from `MAX_TREE_DEPTH`. They serve different purposes (validation cap vs allocation sizing) and their values may diverge over time as profiling data accrues.

**Alternatives considered:**

| Alternative | Why rejected |
|---|---|
| **A. Single constant `MAX_TREE_DEPTH = 256`; derive `INLINE_TREE_DEPTH = MAX_TREE_DEPTH / 8`** | Couples allocation sizing to validation cap. If profiling later raises `MAX_TREE_DEPTH` to 512, inline cap silently doubles to 64 — every `SmallVec<[T; INLINE_TREE_DEPTH]>` stack-frame doubles. The two concerns are orthogonal. The supervisor's brief proposed this; rejected because the audit's evidence (T-10: four constants drifting) was *not* about a single source — it was about coordinated-but-independent values. |
| **B. Single constant `MAX_TREE_DEPTH = 256`; hard-code inline caps inline (`[T; 32]` literals)** | The exact pre-cycle-3 footgun (audit T-10). Forbidden by `tree-depth-canonical/spec.md` R2's anti-regression scenario. |
| **C. Crate-wide const generic parameter `<const D: usize>`** | Viral — every consumer threads `D` everywhere. Defeats the no-monomorphisation-explosion principle FOUNDATIONS.md locks. |
| **D. Runtime config (read from env var)** | Defeats `const fn` everywhere; turns compile-time `[T; N]` into runtime `Vec` allocation. Lose perf for ergonomics no one asked for. |

**Tradeoff resolved:** Two independent named constants give (a) one canonical source per concern, (b) compile-time guarantee `INLINE_TREE_DEPTH ≤ MAX_TREE_DEPTH` via `const _ = assert!(...)`, (c) future-proof independence between validation policy and inline-allocation policy. Cost: contributors must learn which constant to reference (mitigated by doc-comment + port-check.sh's `MAX_DEPTH|MAX_STACK_DEPTH|STACK_SIZE` regex catching drift attempts).

**Spec home:** `tree-depth-canonical/spec.md` R1, R2, R6.

**Verification at sdd-apply:** No code change; `port-check.sh` already gates regressions via FR-033 (`tree-depth-canonical/spec.md` R1 scenario "No drifted depth constants" maps to an existing port-check rule).

**Open question for supervisor:** The brief's draft proposed `INLINE_TREE_DEPTH` as a "derived sub-constant" of `MAX_TREE_DEPTH = 256`. The design rejects derivation in favour of independence. **If the supervisor wants derivation**, the spec changes; this is a 50/50 call on coupling. Default: independence (current shape).

---

### 4.3 — Zombie surface disposition per module (RATIFY)

**Decision (landed cycle 3 PRs #103 + #105; ratified here):**
Every audit-targeted zombie module was **deleted**, not feature-gated. The decision rule was `no-quick-wins-vanyastaff`: feature-gated dead code is still maintenance burden (compiles in CI on every feature combination matrix), still doc-burden (rustdoc renders gated items), and revival from git history is the cleaner path when a real consumer materialises.

**Per-module disposition table:**

| Module | LOC | Action | Rationale | Flutter analog | Future consumer (if revived) |
|---|---|---|---|---|---|
| `crates/flui-tree/src/state.rs` (Mountable/Unmountable typestate) | 616 | **delete** | Two-state generalisation didn't match Flutter's four-state Element FSM; zero in-workspace consumers (Element lifecycle owned by `flui-view/src/element/lifecycle.rs`). | `framework.dart::Element._lifecycleState` (four-state) | If a future cross-tree generic two-state typestate becomes load-bearing, port from git history (pre-PR #105); place behind `unstable-typestate` feature; require real consumer before public re-export. |
| `crates/flui-tree/src/visitor/` (StatefulVisitor, TypedVisitor, ComposedVisitor, FallibleVisitor, 17+ types) | ~2,560 | **delete** | Zero in-workspace consumers. Closure-based `tree.descendants(root).filter(...).collect()` covers the visitor-pattern use case more ergonomically. | per-class `visitChildren(visitor:)` family in `framework.dart` | Future devtools binding (re-enabled `flui-devtools`) needing structured tree-walk callbacks with named states / composition. Port from git history; `unstable-devtools` feature gate. |
| `crates/flui-tree/src/diff.rs` (TreeDiff, DiffOp, ChildDiff, DiffStats, TreeDiffer) | 1,234 | **delete** | Zero consumers. flui-view's actual reconciliation uses per-element key-based child reconciliation, not generic tree-diff. | `framework.dart::Element.updateChild` (per-class diffing, not generic) | Devtools inspector visualisation OR scene-plugin hot-reload diffing. Port + `unstable-devtools`. |
| `crates/flui-tree/src/iter/cursor.rs` | ~1,000 | **delete** | Zero consumers. | None (per-class traversal in Flutter) | Devtools selection / serialization. |
| `crates/flui-tree/src/iter/path.rs` (TreePath, IndexPath, TreeNavPathExt) | ~900 | **delete** | Zero consumers. | None (per-class in Flutter) | Devtools serialization for hot-reload diff payloads. |
| `crates/flui-tree/src/iter/breadth_first.rs` | ~950 | **delete** | Zero consumers; `tree.descendants(root)` already covers depth-first need. | None | Same as cursor / devtools. |
| `crates/flui-tree/src/iter/depth_first.rs` (with `DepthFirstOrder` enum) | ~950 | **delete** | Zero consumers; pre-order vs post-order both rare outside diff. | None | Same as cursor / devtools. |
| `crates/flui-tree/src/arity/storage.rs` (`ChildrenStorage<T, A>` + 11 traits) | ~1,200 | **delete** | flui-rendering uses per-arity-type field pattern (`BoxChild<Single>`) not generic enum storage. | `RenderObjectWithChildMixin<ChildType>` + `ContainerRenderObjectMixin` (Dart mixins) | Generic arity-storage as a runtime enum for hot-reload of arity-typed render-trees. Port; gate. |
| `crates/flui-tree/src/arity/arity_storage.rs` | ~1,000 | **delete** | Same as `storage.rs` — speculative generic. | Same | Same. |
| `crates/flui-tree/src/arity/accessors.rs` | ~800 | **delete** | Same. | Same | Same. |
| `crates/flui-tree/src/arity/runtime.rs` + `arity/aliases.rs` | ~200 | **delete** | Sibling deletions. | Same | Same. |
| `crates/flui-tree/src/traits/node.rs` (`Node` trait + `NodeExt` + `NodeTypeInfo`) | 305 | **delete** | `type Id: Identifier` provides no work; zero external impls. | None (Flutter has per-class Element/RenderObject/Layer types) | A future cross-tree generic algorithm bound on a `Node` trait alias. Trivial re-introduction; not worth keeping dormant. |
| `crates/flui-tree/src/state.rs::MountableExt` | (incl. in state.rs) | **delete** | Sibling of state.rs deletion. | None | If state.rs is revived. |
| `crates/flui-foundation/src/observer.rs` (`ObserverList<T>`) | 271 | **delete** | Zero consumers; `ChangeNotifier` covers observer-pattern needs with O(1) removal by `ListenerId`. See §4.5. | `observer_list.dart::ObserverList<T>` (8 Flutter consumers) | None expected — every FLUI consumer that needs observer collection uses ChangeNotifier or its own `HashMap<Handle, T>`. |
| `crates/flui-foundation/src/error.rs` (`FoundationError`, `ErrorContext`) | 335 | **delete** | Zero consumers; clashed with `anyhow::Context`. | `FlutterError` family (kept conceptually as `thiserror` enums per crate) | Never — `thiserror` enums per concrete crate is the canonical pattern. |
| `crates/flui-foundation/src/consts.rs::approx_equal*` | 60 | **delete** | Zero consumers; relocates to `flui-types` if needed. | None | If a math-heavy crate needs them. |
| `crates/flui-foundation/src/assert.rs::report_error!/report_warning!` | 25 | **delete** | Zero consumers; direct `tracing::warn!` covers the case. | `FlutterError.reportError` family | Never. |
| `crates/flui-foundation/src/wasm.rs::WasmNotSend` | 15 | **delete** | Zero consumers; `WasmNotSendSync` is the sole wasm marker. | None | Never. |

**Kept-with-caveats:**

| Module | LOC | Action | Why kept |
|---|---|---|---|
| `crates/flui-tree/src/traits/ext.rs` (`TreeReadExt`, `TreeNavExt`) | ~260 | **keep** | T-15 partial close: "have real-world ergonomic value" — `find_node_where`, `path_to_node`, `collect_nodes_where` are used by tests and downstream consumers. |
| `crates/flui-tree/src/arity/types.rs` (arity markers: `Leaf`, `Single`, `Optional`, `Variable`, `Exact<N>`, `AtLeast<N>`, `Range<N, M>`, `Never`) + simplified `Arity` trait | ~400 | **keep** | flui-rendering uses these as type-level binding tags on render-object structs. The deletion is specifically the *storage layer*, not the *markers*. |

**Total deletion (cycle 3): ~11,600 LOC. Net workspace LOC delta vs pre-cycle-3: −11,600 + ~200 (new TreeWrite cascade machinery + 2 TreeWrite impls + Flutter ref doc-comments) = ~−11,400 LOC.**

**Alternatives considered for the disposition strategy:**

| Alternative | Why rejected |
|---|---|
| **A. Feature-gate everything under `unstable-devtools`** | Per `no-quick-wins-vanyastaff`: still compiles in CI matrix, still maintenance burden, still rustdoc surface. Plus the gate has no consumer to validate it against — every revival from feature-gated state requires the same "wire up real consumer" work as revival from git history. |
| **B. Move to a separate `flui-tree-unstable` crate** | Adds a workspace member with the same maintenance cost; creates a discoverability problem ("where's the visitor surface?"); breaks the layered DAG simplicity. |
| **C. Keep with `#[deprecated]` annotations** | Compiler warnings on every build; doesn't reduce surface; doesn't unblock the parity audit. |
| **D. Delete (chosen)** | Recovers all maintenance/CI/rustdoc cost; revival from git history is one `git log -p` away; the "future consumer materialises" trigger is the clean re-introduction point. |

**Tradeoff resolved:** Deletion is the highest-clarity, lowest-cost disposition when the revival cost (port one file from git history when a real consumer arrives) is bounded and well-understood. The audit's recommendation column had P0 deletions and P0 feature-gates side-by-side; cycle 3 promoted the feature-gate set to deletions per the supervisor's memory rule. The result is the cleanest crate-pair in the workspace.

**Spec home:** `tree-surface-reduction/spec.md` R1–R7 + `foundation-listenable-changenotifier/spec.md` R7 + amendment to `flui-foundation/ARCHITECTURE.md` Architecture Decision Summary table.

**Verification at sdd-apply:** `! test -e <path>` per deletion (covered by tasks.md verify step T-final).

---

### 4.4 — `BindingBase` init-after-panic hazard fix (RATIFY)

**Decision (landed cycle 3 PR #103; ratified here):**
The `impl_binding_singleton!` macro generates `instance()` body where `INITIALIZED.store(true, Release)` fires **after** `OnceLock::get_or_init(<Self>::new)` returns successfully:

```rust
// Generated by impl_binding_singleton!(MyBinding); — corrected ordering
fn instance() -> &'static Self {
    static INSTANCE: OnceLock<MyBinding> = OnceLock::new();
    let inst = INSTANCE.get_or_init(MyBinding::new);  // panics propagate from new()
    Self::INITIALIZED.store(true, Ordering::Release);   // ONLY reached if get_or_init returned
    inst
}
```

**Why this ordering:**
- If `new()` panics, control unwinds from `get_or_init` **before** the `.store(true)` line. `INITIALIZED` stays `false`. Next caller's `is_initialized()` correctly returns `false`. `INSTANCE` (the OnceLock) also stays empty (Rust's `OnceLock::get_or_init` does not memoise panicking initialisers).
- Steady-state read path takes one redundant atomic Release write per `instance()` call (already-true → still true). Audit notes a future micro-optimisation: `INITIALIZED.compare_exchange(false, true, Release, Relaxed).ok()` to skip the redundant write — left as future tuning, not current contract.

**Alternatives considered:**

| Alternative | Why rejected |
|---|---|
| **A. Pre-cycle-3 shape: store inside the `get_or_init` closure before `new()` returns** | The audit I-3 hazard: if `new()` panics, `INITIALIZED == true` but `OnceLock` is empty. Next `is_initialized() → true` caller observes incoherent state. Critical bug. |
| **B. `INIT_ONCE` (`std::sync::Once`) instead of `OnceLock + AtomicBool`** | `Once` doesn't store a value; would need a parallel `static mut INSTANCE: MaybeUninit<Self>` with unsafe init. `OnceLock` is the Rust-1.70+ idiomatic shape that wraps both concerns. |
| **C. Drop `INITIALIZED` entirely; use `INSTANCE.get().is_some()`** | Two indirection levels per `is_initialized()` call (load OnceLock state, then check `Option<&T>`). The `AtomicBool` is the cheap shape (single Acquire load). |
| **D. `compare_exchange` (future micro-optimisation)** | Worth doing when a profile shows the redundant Release write matters. Not the current bottleneck per audit Mythos verdict; left as `## Outstanding refactors` item if the parity sweep surfaces it. |

**Tradeoff resolved:** `OnceLock + AtomicBool` two-element shape preserves panic-safety, allows zero-cost steady-state `is_initialized()` reads via `Acquire` load, and the macro's per-binding-scoped statics keep it consumer-friendly. The regression test (`binding.rs::tests::init_panic_does_not_flip_initialized_flag`) is the binding-tier proof.

**Spec home:** `foundation-binding/spec.md` R3 (canonical macro contract scenario `Panicking init leaves INITIALIZED at false`).

---

### 4.5 — `ObserverList<T>` decision (RATIFY)

**Decision (landed cycle 3 PR #103; ratified here):**
`ObserverList<T>` is **deleted**. `observer.rs` does not exist; `lib.rs` does not declare `pub mod observer;`; the prelude does not re-export `ObserverList`.

**Brief's three options, with verdict:**

| Option | Verdict | Rationale |
|---|---|---|
| **(a) delete** (no FLUI consumers; ChangeNotifier covers) | **CHOSEN** | Zero in-workspace consumers verified at audit Appendix A.2 grep. Every FLUI need is met by `ChangeNotifier` (O(1) removal by `ListenerId`, snapshot-then-fire reentrancy) or by ad-hoc `HashMap<Handle, T>` in the consumer (`flui-scheduler::Ticker` registrations, `flui-layer` layer-link bookkeeping). |
| **(b) keep as Flutter-faithful port awaiting future consumer** | rejected | `no-quick-wins-vanyastaff` rule: maintenance + CI compile burden with no validation. Revival from git history when needed is the cleaner path. |
| **(c) refactor ChangeNotifier to use ObserverList internally** | rejected | Flutter does *not* do this (Flutter's `ChangeNotifier` uses its own `_listeners` array, NOT `ObserverList`). Adopting it would invent a divergence in the parity story. Plus FLUI's `HashMap<ListenerId, ListenerCallback>` provides O(1) removal that an array-backed `ObserverList` would degrade. |

**Evidence for (a):**
- `docs/research/2026-05-22-flui-foundation-tree-audit.md` Appendix A.2: zero in-workspace `use flui_foundation::ObserverList` references at audit time.
- Flutter's 8 in-tree consumers all live in animation/widgets/services packages — none of those concepts are direct FLUI ports (animation deferred until re-enable; widgets has its own observer machinery in flui-view; services has no FLUI equivalent yet).
- `crates/flui-scheduler/src/ticker.rs` Ticker callbacks use `HashMap<TickerId, Callback>` directly — the FLUI-native pattern.

**Tradeoff resolved:** Delete + `no-quick-wins-vanyastaff` rule + clean revival from git history when a real consumer materialises. The parity-verification report (§9) flags `ObserverList` as a **deliberate divergence** with explicit cross-reference to this decision.

**Spec home:** `foundation-listenable-changenotifier/spec.md` R7.

**Open question for supervisor:** If the supervisor disagrees and wants option (b) "keep as Flutter-faithful port", this is a contingent code change for sdd-apply (re-introduce `observer.rs` from git history, place behind `unstable-observer-list` feature gate). **Default: option (a) — keep deleted.**

---

### 4.6 — Thirteen deferred audit findings: verdict table (RATIFY)

This is the canonical verdict table for the 13 findings the cycle-3 audit deferred. Verdicts mirror the per-domain specs' draft verdicts; this table is the single-source ratification artifact for cross-spec consistency.

Verdict enum: `accept-permanent` (closed without code change), `revisit-now` (open follow-on code task in this change, under strict TDD), `revisit-later-with-trigger` (record trigger condition in the relevant ARCHITECTURE.md `## Outstanding refactors` ledger).

| Audit § | Finding (one-line) | Verdict | Spec home | Trigger (if revisit-later) | Mirror destination |
|---|---|---|---|---|---|
| **I-6** | `Key::from_str` collision-with-zero fallback (silent hash collision possible) | **accept-permanent** | foundation-key R2 | — | foundation/ARCHITECTURE.md `## Mapping decisions` |
| **I-7** | `Key::try_new` Result-returning ctor (off-by-one overflow recovery) | **revisit-later-with-trigger** | foundation-key R3 | A workspace consumer materialises that needs to recover from `Key::new()` counter overflow without panicking | foundation/ARCHITECTURE.md `## Outstanding refactors` |
| **I-8** | `ViewKey::is_global_key()` abstract (no default-false) | **accept-permanent** | foundation-key R6 | — | foundation/ARCHITECTURE.md `## Mapping decisions` |
| **I-9** | `Id<T>::from_raw` / `zip_unchecked` / `new_unchecked` `pub(crate)` (currently `pub`) | **revisit-later-with-trigger** | foundation-id-system R6 | A cycle 4+ workspace audit migrates `flui-scheduler::id::*` off the public `unsafe` constructors | foundation/ARCHITECTURE.md `## Outstanding refactors` |
| **I-10** | `RawId` + `Index` `pub(crate)` (currently `pub`) | **revisit-later-with-trigger** | foundation-id-system R7 | Same as I-9 | foundation/ARCHITECTURE.md `## Outstanding refactors` |
| **I-12** | Sweep doc-comments to cite Flutter file:line uniformly | **revisit-later-with-trigger** | foundation-binding R5 | The §9 parity-verification sweep discovers a divergence whose root cause is a missing file:line ref leading to drift | foundation/ARCHITECTURE.md `## Outstanding refactors` |
| **I-15** | `ChangeNotifier::has_listeners` / `is_empty` / `len` via lock-free `AtomicUsize` | **accept-permanent** | foundation-listenable R9 | — | foundation/ARCHITECTURE.md `## Mapping decisions` (cross-ref existing `Notifier` row) |
| **I-17** | `ValueNotifier::take` / `replace` / `value_mut` audit / mark unused | **accept-permanent** | foundation-listenable R5 + foundation-id-system R8 | — | foundation/ARCHITECTURE.md `## Mapping decisions` |
| **I-18** | `Marker` trait drop `+ Debug` supertrait | **accept-permanent** | foundation-id-system R3, R9 | — | foundation/ARCHITECTURE.md `## Mapping decisions` |
| **I-21** | Deprecate `KeyRef::new` in favor of `From<Key>` | **accept-permanent** | foundation-key R9 | — | foundation/ARCHITECTURE.md `## Mapping decisions` |
| **T-17** | `Slot::with_siblings` positional → `bon` builder | **revisit-later-with-trigger** | tree-surface-reduction R9 | A workspace-wide pass adopts `bon` builders for multi-arg constructors elsewhere AND the cost aligns with `Slot::with_siblings` conversion | tree/ARCHITECTURE.md `## Outstanding refactors` |
| **T-19** | `TreeNav::depth` slow-default doc + recommend override | **revisit-later-with-trigger** | tree-depth-canonical R7 | A profile shows `TreeNav::depth` is a hot path on an impl with stored depth that forgot to override, OR a doc-cleanup pass standardises slow-default markings across the trait surface | tree/ARCHITECTURE.md `## Outstanding refactors` |
| **T-24** | `Descendants::new` / `Ancestors::new` / `Siblings::new` etc. `pub(crate)` | **revisit-later-with-trigger** | tree-surface-reduction R10 | A workspace-wide audit shows every concrete TreeNav impl constructs these via the trait method (not the constructor) AND no consumer outside `flui-tree` itself uses the constructor | tree/ARCHITECTURE.md `## Outstanding refactors` |

**Aggregate:** 6 `accept-permanent` (I-6, I-8, I-15, I-17, I-18, I-21) + 7 `revisit-later-with-trigger` (I-7, I-9, I-10, I-12, T-17, T-19, T-24) + 0 `revisit-now`.

**Alternatives considered for the aggregate verdict shape:**

| Alternative | Why rejected |
|---|---|
| **A. Flip I-7 (`Key::try_new`) to `revisit-now`** | Adds public API surface (`fn try_new() -> Result<Self, KeyOverflow>`) with no current consumer. The 584-years-at-1-ns assertion-panic is the pragmatic shape. The proposal's RP2 budgeted ≤3 such tasks; spending one on I-7 with no callsite cost is premature. |
| **B. Flip I-8 (`is_global_key` abstract) to `revisit-now`** | Forces 3+ key impls (`ValueKey`, `UniqueKey`, `flui-view::ObjectKey`, `flui-view::GlobalKey`) to write explicit `fn is_global_key() { false }` for cosmetic reasons. The default-false safety net catches the "forgot to override" case identically. |
| **C. Flip I-21 (`KeyRef::new` deprecation) to `revisit-now`** | `#[deprecated]` warnings on every build + migration cost on consumers that haven't yet adopted `Key::into()`. The migration is the right move when a workspace-wide refactor consolidates on `Into<KeyRef>`; doing it now produces warning noise without value. |
| **D. Flip all 13 to `accept-permanent`** | The 7 `revisit-later-with-trigger` items have well-defined triggers (e.g. I-9 + I-10 tied to `flui-scheduler` audit; I-12 tied to parity sweep). Recording the trigger is more informative than declaring "permanent" — the next audit can check whether the trigger has fired. |
| **E. Flip all 13 to `revisit-later-with-trigger`** | Six of the items have *no* well-defined trigger — they're judgment calls the audit explicitly recommended accepting. "Accept-permanent with no trigger" is the correct verdict for those. |

**Tradeoff resolved:** Per-finding analysis (each verdict has its own rationale chain). The aggregate split (6 accept-permanent / 7 revisit-later-with-trigger / 0 revisit-now) matches the proposal §2.1's expected distribution ("most → accept-permanent, possibly 1–3 → revisit-now"). The 0-revisit-now choice is conservative; the parity-sweep deliverable (§9) is the gate that could flip any of the 7 revisit-later items if it surfaces real divergence.

**Verification at sdd-apply:** The `tree-architecture-md/spec.md` R7 scenario asserts that each of T-17, T-19, T-24 appears in `crates/flui-tree/ARCHITECTURE.md ## Outstanding refactors`. Similarly, the foundation `## Outstanding refactors` amendment (PR2) MUST list I-7, I-9, I-10, I-12.

**Open question for supervisor:** This entire table is the most plausible adjudication point. **If the supervisor wants any verdict flipped to `revisit-now`**, that becomes a sdd-apply code task with strict-TDD (RED test commit → GREEN fix → optional REFACTOR). The proposal §3.1 reserved up to 3 such tasks plus up to 2 parity-divergence-driven tasks (total ≤5 contingent code tasks, all ≤400-line review budget).

---

## 5. Migration plan — PR sequencing across the apply phase

**Default path (verdict table holds, parity sweep clean):**

| PR | Tasks | Lines | Tests | Strict-TDD requirement | Self-contained? |
|---|---|---|---|---|---|
| **PR1** | T1: Author `crates/flui-tree/ARCHITECTURE.md` (NEW; 5 fixed sections per PORT.md template + audit cross-ref + AtomicDepth thread-safety note) | ~400-500 added | n/a (doc-only) | n/a (no code change → no test required) | yes (doc-only; no downstream rebuild) |
| **PR2** | T2a: Amend `crates/flui-foundation/ARCHITECTURE.md` (Architecture Decision Summary table: delete ObserverList/FoundationError/WasmNotSend rows; add `log/` row; update `Notifier` row to mention SmallVec inline-4 + `Arc<AtomicBool>` disposed; add `## Mapping decisions` entries for cycle-3 deletions; add `## Outstanding refactors` entries for I-7, I-9, I-10, I-12)<br>T2b: Flip `docs/PORT.md` Index row for `flui-tree` from "Not yet templated" → `Templated 2026-05-25` (or commit-day's ISO date) | ~80-120 added/modified + 1-line edit | n/a (doc-only) | n/a | yes (doc-only; depends on PR1's file existing to cite if PR1 lands first; otherwise can land in either order — see below) |
| **PR3** | T3: Author `docs/research/2026-XX-XX-foundation-parity-verification.md` (NEW; ~200-400 lines; one table row per Flutter foundation type per the row template in §9) | ~250-350 added | n/a (doc-only) | n/a | yes |

**PR ordering rationale:**
- **PR1 first** because `flui-tree/ARCHITECTURE.md` is the new artifact that PR2's `## Outstanding refactors` cross-references (PR2 mentions "see `crates/flui-tree/ARCHITECTURE.md ## Outstanding refactors` for T-17/T-19/T-24"). Reverse order is permissible (forward refs are fine in markdown) but creates a window where the cross-reference dangles.
- **PR2 second** because the PORT.md Index flip should land alongside the foundation/ARCHITECTURE.md amendment (single commit theme: "foundation ARCH ledger sync").
- **PR3 last** because the parity-verification doc cites both ARCHITECTURE.md files (and any divergence it discovers becomes a contingent PR4+).

**Contingent path (parity sweep discovers divergence, OR §4.6 verdict flipped to `revisit-now`):**

| PR | Tasks | Lines | Tests | Strict-TDD requirement |
|---|---|---|---|---|
| PR4 (if needed) | Surgical code fix for divergence #1 (e.g. I-7 `Key::try_new`) | ≤400 | NEW regression test for divergence | RED commit (failing test asserting parity) → GREEN commit (impl) → optional REFACTOR commit |
| PR5 (if needed) | Surgical code fix for divergence #2 (e.g. I-8 `is_global_key` abstract) | ≤400 | NEW regression test | Same TDD shape |
| PR6 (if needed) | Surgical code fix for divergence #3 (e.g. I-21 `KeyRef::new` deprecation) | ≤400 | n/a (deprecation attr only) | RED check that callers see the warning → GREEN attribute |

**Hard cap on contingent PRs: 5** (per proposal §3.1: "Up to 3 surgical code tasks ... Up to 2 surgical code tasks if parity sweep discovers ...").

**Why this PR count vs the brief's 4–6 PR ask:**
- The brief sized 4-6 PRs against a fresh code change. Post-cycle-3 the code is already in `main`. Default path is 3 doc PRs (well under the 4-6 ceiling), totalling ~1,200 review lines (well under the 4,000-line session review budget).
- The contingent path adds up to 5 code PRs, each strictly ≤400 lines, totalling worst-case ~5,200 review lines across 8 PRs. This *exceeds* the 4,000-line session budget — but only in the unlikely all-contingent-paths-fire scenario. The verify gate (SC6 `just ci`) is per-PR not per-session, so the budget violation is procedural-only.

**RED → GREEN → TRIANGULATE → REFACTOR evidence template (for any contingent code PR):**

```
PR-N commits (in order):
1. test(<domain>): RED — assert <observable Flutter parity behaviour>
   - Files: crates/<crate>/tests/<test_name>.rs
   - Asserts the cycle-3-closed expectation OR the parity-sweep-discovered divergence resolution
   - CI exits 1 with the new test failing
2. <type>(<domain>): GREEN — implement <minimal change>
   - Files: crates/<crate>/src/<file>.rs
   - Smallest possible change to flip the RED test green
   - CI exits 0
3. (optional) test(<domain>): TRIANGULATE — add second test exercising adjacent invariant
   - Files: same test file as commit 1
   - CI exits 0
4. (optional) refactor(<domain>): REFACTOR — clean up duplication / naming surfaced by GREEN+TRIANGULATE
   - Files: src + tests
   - CI exits 0
```

The verify task asserts every contingent code PR has at least commits 1+2 (RED→GREEN) per SC5.

---

## 6. Breaking change inventory

**Default path (verdict table holds, parity sweep clean):**

| Change | Downstream crates affected | Mechanical fix |
|---|---|---|
| `docs/PORT.md` Index row flip (1 line) | none (doc-only; no crate consumes PORT.md at compile time) | n/a |
| `crates/flui-tree/ARCHITECTURE.md` add | none (new doc-only file) | n/a |
| `crates/flui-foundation/ARCHITECTURE.md` amend | none (doc-only; rustdoc generation may pick up new file but does not break) | n/a |
| `docs/research/2026-XX-XX-foundation-parity-verification.md` add | none (research doc) | n/a |

**Total downstream breakage in default path: ZERO.**

**Contingent path (per any flipped-to-`revisit-now` verdict):**

| Change | Downstream crates affected | Mechanical fix |
|---|---|---|
| I-7 `Key::try_new` added (additive) | none (additive `pub fn`) | n/a; `Key::new` retains existing signature |
| I-8 `ViewKey::is_global_key()` made abstract (no default) | `flui-foundation::key::{ValueKey, UniqueKey}`, `flui-view::key::{ObjectKey, GlobalKey}` (≥4 impls) | Add `fn is_global_key(&self) -> bool { false }` to each `impl ViewKey for` block; `GlobalKey<T>` returns `true`. Each impl is ~3 lines. Total: ~12-15 lines across 4 files. |
| I-21 `KeyRef::new` deprecation | call sites of `KeyRef::new` (verified ≥2 internal; zero external workspace consumers via grep) | Replace `KeyRef::new(k)` with `KeyRef::from(k)`. Each site is a 1-line edit. |
| Parity divergence #1 (unknown until sweep) | TBD by sweep | TBD by sweep; budgeted ≤400 lines per PR |
| Parity divergence #2 (unknown until sweep) | TBD by sweep | TBD by sweep; budgeted ≤400 lines per PR |

**Mechanical-fix discipline:** any breaking change PR includes the mechanical fix in the same PR (per `openspec/config.yaml rules.tasks.protect_review_workload` — keep mechanical fix in the introducing PR so reviewer sees the full impact in 400 lines).

---

## 7. `port-check.sh` additions for refusal triggers #8–#13

**Status:** **landed PR #151.** The 13 triggers are installed at `scripts/port-check.sh:396-1005` (verified via `Grep` "port-check" check producing FR-033 + FR-036 references). No further additions required by this change.

The relevant trigger-to-implementation table:

| Trigger | Implementation in port-check.sh | Whitelist marker |
|---|---|---|
| #8 SP-1 `From<scalar>` escape hatch on wrappers | `port-check.sh:347-391` (DOWNCAST check) | `// PORT-CHECK-OK-DOWNCAST: <reason>` |
| #9 SP-2 `todo!()` / `unimplemented!()` STUB | `port-check.sh:431-459` (STUB check) | `// PORT-CHECK-OK-STUB: <reason + tracking-issue>` |
| #10 SP-3 speculative API surface on shipped types | `port-check.sh:518-557` (SP-3 check) | `// PORT-CHECK-OK-SP3: <reason + tracking-issue>` |
| #11 SP-4 speculative `pub mod` additions | `port-check.sh:627-653` (SP-4 check) | `// PORT-CHECK-OK-SP4: <reason + tracking-issue>` |
| #12 SP-6 speculative trait signatures | `port-check.sh:732-759` (SP-6 check) | `// PORT-CHECK-OK-SP6: <reason + tracking-issue>` |
| #13 SP-8 `unwrap()` in shipped code | `port-check.sh:800-812` (SP-8 check) | `// PORT-CHECK-OK-SP8: <reason>` |

**Verification at sdd-apply (SC7):** `bash scripts/port-check.sh -v` exits 0 with `"port-check: all 13 refusal triggers + FR-033 grep clean"`.

**Open friction item (low severity):** the existing port-check has no rule explicitly covering the `MAX_DEPTH | MAX_STACK_DEPTH | STACK_SIZE | STACK_DEPTH | MARK_PROPAGATION_MAX_DEPTH` constant-drift regex from `tree-depth-canonical/spec.md` R1's anti-regression scenario. If the parity sweep determines this matters, a `FR-037` rule could be added. **Not required by this design** — the spec scenario expresses the assertion and can be verified by a single grep in the verify task without needing port-check.sh extension.

---

## 8. Engram protocol for the apply phase

**Status:** Engram `mem_save` is NOT exposed in this sdd-plan session's tool list (verified by the spec-step envelope: "Engram `mem_save` is NOT exposed in this session's tool list"). Each phase has documented inline persistence as fallback. The apply phase MAY have Engram tools — protocol below covers both cases.

**If Engram is available at apply:**

Save one observation per PR with stable topic keys:

| Topic key | Saved on | Content shape |
|---|---|---|
| `sdd/core-0a-foundation-parity-to-flutter/apply-progress` | After each PR merge | YAML: `pr: <N>, files_added: [...], files_modified: [...], audit_findings_closed: [I-N, T-N, ...], strict_tdd_evidence: <commit-shas>, ci_status: green` |
| `sdd/core-0a-foundation-parity-to-flutter/verify-report` | After final verify task | YAML: `sc1..sc12 status, port-check status, just ci status, total review lines per PR` |
| `sdd/core-0a-foundation-parity-to-flutter/deferred-13-verdicts` | After PR2 merge | Final verdict table copy from §4.6 (durable cross-cycle reference) |
| `sdd/core-0a-foundation-parity-to-flutter/parity-sweep-divergences` | After PR3 merge | List of divergences discovered (default: empty); each entry carries Flutter ref + FLUI counterpart + resolution-PR if any |
| `sdd/core-0a-foundation-parity-to-flutter/design` | Once at design-merge time | This file's content (in case ledger reconstruction is needed) |

**If Engram is unavailable at apply:**

Mirror each topic key's content into:
- The chain-run progress file at `C:\Users\vanya\AppData\Local\Temp\pi-subagents-user-vanya\chain-runs\949e3e92\progress.md` (rolling update per PR).
- A new section in `crates/flui-tree/ARCHITECTURE.md ## Outstanding refactors` for the deferred-13 mirror (already required by `tree-architecture-md/spec.md` R7).
- A new section in `crates/flui-foundation/ARCHITECTURE.md ## Outstanding refactors` for the foundation-side deferred items (already required by amendment R10).
- A new "Conformance log" appendix section in the parity-verification research doc for sc1..sc12 status (added at PR3 authoring time).

**Strict-TDD evidence preservation:**
- For each contingent code PR: tag the RED-commit SHA + GREEN-commit SHA in the PR description + Conformance log.
- For each parity-sweep-discovered divergence: add a row in the parity-verification research doc's "Discovered divergences" appendix table with Flutter ref + before-fix shape + after-fix shape + resolution PR + regression-test name.

---

## 9. Parity-verification report row template + per-Flutter-type cross-reference

The §2.3 deliverable (parity-verification report at `docs/research/2026-XX-XX-foundation-parity-verification.md`, date set at PR3 author time) has this row template per Flutter foundation type:

```markdown
### <FlutterTypeName>

| Field | Value |
|---|---|
| Flutter type | `<ClassName>` |
| Flutter source | `.flutter/packages/flutter/lib/src/foundation/<file>.dart:LINE-LINE` |
| FLUI counterpart | `<RustTypeName>` |
| FLUI source | `crates/<crate>/src/<file>.rs:LINE-LINE` |
| Observable-behavior tests | `crates/<crate>/tests/<test_file>.rs::<test_name>` (× N) |
| Divergence (if any) | One of: `none / minor-shape-only / deliberate-design / behaviour-bug` |
| Divergence rationale | Cross-ref to `crates/<crate>/ARCHITECTURE.md ## Mapping decisions` entry |
| Audit cross-ref | Cycle-3 audit finding(s) that touched this type, e.g. `I-1, I-4` |
| Verdict | `parity-faithful / deliberately-diverged / divergence-fix-PR-N` |
```

**Cross-reference table (skeleton; PR3 fills the LINE-LINE ranges and test names):**

| Flutter type | FLUI counterpart | FLUI home | Audit cross-ref | Expected verdict |
|---|---|---|---|---|
| `Listenable` (`change_notifier.dart:60-94`) | `Listenable` | `flui-foundation/src/notifier.rs` | I-1, I-16 | deliberately-diverged (`Send + Sync` super-trait + `remove_all_listeners` added) |
| `ChangeNotifier` (`change_notifier.dart:139-465`) | `ChangeNotifier` | `flui-foundation/src/notifier.rs` | I-4, I-15, I-20 | deliberately-diverged (HashMap vs array; `SmallVec<[CB; 4]>` snapshot vs in-place iteration) |
| `ValueListenable<T>` (`change_notifier.dart:467-482`) | `ValueListenable<T>` | `flui-foundation/src/notifier.rs` | post-cycle | parity-faithful |
| `ValueNotifier<T>` (`change_notifier.dart:484-525`) | `ValueNotifier<T>` | `flui-foundation/src/notifier.rs` | I-17, I-20 | deliberately-diverged (`take`/`replace`/`value_mut` FLUI-native; `into_value` disposes) |
| `VoidCallback` (`change_notifier.dart:70`) | `VoidCallback` alias | `flui-foundation/src/callbacks.rs` | I-16 | parity-faithful (`Arc<dyn Fn() + Send + Sync + 'static>` per Rust idiom) |
| `Key` (`key.dart:29-48`) | `Key` | `flui-foundation/src/key.rs` | I-5, I-6, I-7, I-21 | deliberately-diverged (`NonZeroU64` + const FNV-1a hash) |
| `LocalKey` (`key.dart:51-58`) | (collapsed into `ViewKey` trait) | `flui-foundation/src/key.rs` | I-8 | deliberately-diverged (FLUI does not split LocalKey/GlobalKey hierarchies; `ViewKey::is_global_key()` discriminator instead) |
| `ValueKey<T>` (`key.dart:88-126`) | `ValueKey<T>` | `flui-foundation/src/key.rs` | (none) | parity-faithful |
| `UniqueKey` (`key.dart:61-83`) | `UniqueKey` | `flui-foundation/src/key.rs` | I-5 | deliberately-diverged (counter-bumped `Key` vs per-instance allocation) |
| `ObjectKey` (`framework.dart`) | `ObjectKey<T>` | `flui-view/src/key/object_key.rs` | (none — view-layer) | parity-faithful (layered to view per crate-graph contract) |
| `GlobalKey<T>` (`framework.dart`) | `GlobalKey<T>` | `flui-view/src/key/global_key.rs` | I-8 | deliberately-diverged (`is_global_key()` virtual dispatch vs Dart `is` type check) |
| `Diagnosticable` (`diagnostics.dart`) | `Diagnosticable` trait | `flui-foundation/src/diagnostics.rs` | I-11, I-19 | parity-faithful + `#[non_exhaustive]` on enums |
| `BindingBase` (`binding.dart:148-321`) | `BindingBase` trait + `HasInstance` + `impl_binding_singleton!` macro | `flui-foundation/src/binding.rs` | I-3, I-12 | deliberately-diverged (trait + macro composition vs Dart mixin chain; post-success `INITIALIZED.store`) |
| `ObserverList<T>` (`observer_list.dart`) | (DELETED — none) | n/a (deleted in cycle 3 PR #103) | I-1 | deliberately-diverged — see §4.5 verdict |

**Total: 14 Flutter foundation types tracked.** The PR3 task fills in the LINE-LINE ranges and the test names; the design's contract is that this table SHALL be present in the research doc.

**Divergence-fix protocol:** if the sweep flips any "expected verdict" from `parity-faithful` or `deliberately-diverged` to `behaviour-bug`, that triggers a contingent code PR per §5's contingent path with strict TDD.

---

## 10. Mapping to spec acceptance criteria (SC1–SC12)

This table binds each proposal-level success criterion to the spec scenario(s) that verify it. Reproduced from proposal §7 with the cross-references.

| SC | Description | Verifying spec scenario(s) | Verify-step command |
|---|---|---|---|
| SC1 | `crates/flui-tree/ARCHITECTURE.md` exists with all 5 sections | tree-architecture-md R1, R2 | `test -f` + `grep -cE` |
| SC2 | `docs/PORT.md` reports `flui-tree` as "Templated" | tree-architecture-md R10 | `grep -E` |
| SC3 | Each of 13 deferred findings has a verdict ∈ enum | §4.6 table + per-spec verdict requirements | inventory grep in design.md table |
| SC4 | Parity-verification doc exists with rows per Flutter type | §9 + tree-architecture-md ARCHITECTURE.md cross-ref | `test -f` + row-count grep |
| SC5 | Strict TDD: code-change commit has preceding RED | n/a default path (zero code change); applies only to contingent PRs | `git log --oneline` per PR |
| SC6 | `just ci` exits 0 | every spec's compilation scenarios | `just ci` |
| SC7 | `bash scripts/port-check.sh -v` exits 0 | §7 + every spec's PORT.md-discipline scenarios | `bash scripts/port-check.sh -v` |
| SC8 | No `unimplemented!()` / `todo!()` in foundation or tree src | `port-check.sh` STUB rule | `! grep -rEn` |
| SC9 | No re-introduction of cycle-3-deleted modules | tree-surface-reduction R1–R6 scenarios + foundation-listenable R7 | per-path `! test -e` |
| SC10 | PORT.md refusal-trigger discipline (#8–#13) | `port-check.sh` (SC7) | covered by SC7 |
| SC11 | Audit doc referenced from both ARCHITECTURE.md files | tree-architecture-md R9 + foundation/ARCHITECTURE.md amendment | `grep -l` both files |
| SC12 | Review-budget: no task exceeds 400 changed lines | §5 PR-sizing table | per-task `git diff --shortstat` |

**Verification mode for SC5:** by default this SC is vacuously satisfied (no code change → no RED-before-GREEN requirement). If §4.6's verdict table flips any item to `revisit-now`, SC5 becomes binding for that PR with the §5 evidence template.

---

## 11. Review & judgment risks (explicit)

| # | Risk | Likelihood | Mitigation in this design |
|---|---|---|---|
| **RD1** | Supervisor disagrees with §4.6 verdict table and wants 1+ items flipped to `revisit-now` | Medium | §4.6 explicitly surfaces this as an open question. §5's contingent path is pre-budgeted for ≤3 verdict-flips. Strict-TDD evidence template is in §5 ready to use. |
| **RD2** | Supervisor disagrees with §4.3 disposition (wants feature-gate over delete for some module) | Low | §4.3's alternatives table explicitly considers feature-gate and rejects it per `no-quick-wins-vanyastaff`. If overridden, the contingent path adds a code PR per module to re-introduce from git history with `unstable-*` gate (~600 lines per module; would breach review budget — supervisor needs to chunk). |
| **RD3** | §9 parity sweep discovers a behaviour-bug divergence in cycle-3-closed surface | Medium | Pre-budgeted as ≤2 contingent PRs per proposal §3.1. Strict-TDD evidence template in §5. The §9 verdict column explicitly carries `behaviour-bug` as one option. |
| **RD4** | Authoring `crates/flui-tree/ARCHITECTURE.md` (PR1) surfaces a friction-log item that should be coded now | Medium | The new file's `## Friction log` section per `tree-architecture-md/spec.md` R5 explicitly accepts two dispositions: in-scope code task OR out-of-scope-with-trigger. The first option routes to a contingent PR; the second stays in the ledger. |
| **RD5** | The two-constant depth shape (§4.2) is rejected by supervisor in favour of derivation | Medium | §4.2 explicitly surfaces this as an open question with the proposed default. If overridden, `tree-depth-canonical/spec.md` R1 needs revision before sdd-apply; this is a spec-step revision, not a code change. |
| **RD6** | `port-check.sh` `MAX_TREE_DEPTH` regex rule not added (§7's open friction item) leaves a regression vector | Low | The `tree-depth-canonical/spec.md` R1 anti-regression scenario expresses the assertion; tasks.md verify-step can grep without port-check.sh extension. If a regression slips past the verify gate, it surfaces in the next audit cycle. |
| **RD7** | Engram unavailability at apply phase causes loss of strict-TDD evidence | Low | §8's "Engram-unavailable fallback" mirrors every topic key into doc artifacts (progress file + ARCHITECTURE.md outstanding sections + parity-verification appendix). |
| **RD8** | Parity-verification doc (§9 / PR3) date stays as `2026-XX-XX` after merge | Low | Tasks.md verify step asserts file exists with a date string matching the change-merge commit's day or earlier; the author-time substitution makes this trivial. |
| **RD9** | Multi-vendor peer review was skipped (per §3 rationale), and a downstream reviewer feels it should have been invoked | Low | §3 explicitly records the decision and rationale. Supervisor can override at design-review gate; if invoked, the cycle becomes Codex (Rust ownership angle) + Gemini (broad context) on the `TreeWrite::remove` contract (§4.1) and `ObserverList` decision (§4.5) — both decisions are already shipped, so the peer-review is post-hoc validation rather than pre-decision input. |

---

## 12. Open questions for supervisor adjudication

1. **§4.2 depth-constant shape** — keep two independent constants (`MAX_TREE_DEPTH = 256` + `INLINE_TREE_DEPTH = 32`, design's default) OR move to derived sub-constant per the original brief (`INLINE_TREE_DEPTH = MAX_TREE_DEPTH / 8`)?

2. **§4.6 deferred-13 verdict table** — accept the 6-accept-permanent / 7-revisit-later-with-trigger / 0-revisit-now split (design's default), OR flip any specific finding to `revisit-now` (most plausible candidates per the audit's deferral severities: I-7 `Key::try_new`, I-8 `is_global_key` abstract, I-21 `KeyRef::new` deprecation)?

3. **§4.5 ObserverList decision** — keep deleted (design's default, cycle-3 ratified) OR re-introduce as Flutter-faithful port behind `unstable-observer-list` feature gate?

4. **§3 peer-review rationale** — accept that peer review is skipped because decisions have landed (design's default) OR require a Codex/Gemini broadcast on the TreeWrite cascade contract + ObserverList decision (post-hoc validation)?

5. **§5 PR ordering** — accept PR1 (tree/ARCHITECTURE.md) → PR2 (foundation/ARCHITECTURE.md + PORT.md) → PR3 (parity-verification doc) (design's default) OR collapse PR1+PR2 into one PR (single doc-ledger update theme; ~580 lines, still under 600)?

6. **§7 port-check.sh `MAX_DEPTH` regex rule** — leave to the spec's R1 anti-regression scenario + tasks.md verify-step grep (design's default) OR add as FR-037 in port-check.sh for permanent regression protection?

If the supervisor does not respond on these by the design-review gate, the design's default for each stands and proceeds to tasks.md authoring.

---

## 13. Implementation notes for the tasks.md writer

The tasks.md author should chunk the work as:

- **T1** — PR1: Author `crates/flui-tree/ARCHITECTURE.md`. Single doc commit. Lines: ≤500. Verify scenarios: tree-architecture-md R1, R2, R3, R4, R5, R6, R7, R9.
- **T2** — PR2: Amend `crates/flui-foundation/ARCHITECTURE.md` + flip `docs/PORT.md` Index row. Single commit. Lines: ≤120. Verify scenarios: tree-architecture-md R8, R10, R11.
- **T3** — PR3: Author `docs/research/<date>-foundation-parity-verification.md`. Single doc commit. Lines: ≤350. Verify: §9 row template adherence + 14-row table.
- **T4..T6 (contingent)** — Each is RED commit → GREEN commit → optional REFACTOR commit. Each ≤400 lines. Only created if §4.6 verdicts flip OR §9 sweep finds divergences.
- **T-final (verify)** — Single verify step running SC1–SC12 commands; output captured in the tasks.md `## verify` section; assertions:
  - `test -f crates/flui-tree/ARCHITECTURE.md` (SC1)
  - `grep -cE '^## (Flutter source mapping|Mapping decisions|Thread safety|Friction log|Outstanding refactors)$' crates/flui-tree/ARCHITECTURE.md` = 5 (SC1)
  - `grep -E '^\|\s*\`flui-tree\`\s*\|\s*Templated' docs/PORT.md` exits 0 (SC2)
  - 13-finding inventory grep against this design.md §4.6 table (SC3)
  - `test -f docs/research/*-foundation-parity-verification.md` (SC4)
  - `git log --oneline` per code PR shows RED→GREEN pair (SC5; vacuous default)
  - `just ci` exits 0 (SC6)
  - `bash scripts/port-check.sh -v` exits 0 (SC7)
  - `! grep -rEn 'unimplemented!\(\)|todo!\(\)' crates/flui-foundation/src crates/flui-tree/src` (SC8)
  - Per-path `! test -e` for the 12 cycle-3-deleted modules (SC9; mirrors §4.3 disposition table)
  - port-check.sh covers (SC10)
  - `grep -l '2026-05-22-flui-foundation-tree-audit' crates/flui-{foundation,tree}/ARCHITECTURE.md` returns both (SC11)
  - Per-task `git diff --shortstat` ≤ 400 changed lines (SC12)
  - **Implicit-coverage findings asserted by the verify step (per spec-step risk #1):** `! test -e crates/flui-foundation/src/observer.rs`, `! test -e crates/flui-foundation/src/error.rs`, `! grep -rn 'FoundationError' crates/flui-foundation/src crates/flui-foundation/examples`, `grep -E '#\[non_exhaustive\]' crates/flui-foundation/src/diagnostics.rs` matches both `DiagnosticLevel` and `DiagnosticsTreeStyle`, `grep 'Box<str>' crates/flui-foundation/src/diagnostics.rs` matches `ParseDiagnostic*Error`, `grep 'Box<str>' crates/flui-tree/src/error.rs` matches `TreeError::Internal`, `! test -e crates/flui-tree/src/visitor`, `! test -e crates/flui-tree/src/iter/cursor.rs`, `! test -e crates/flui-tree/src/iter/path.rs`, `! test -e crates/flui-tree/src/iter/breadth_first.rs`, `! test -e crates/flui-tree/src/iter/depth_first.rs`, `! test -e crates/flui-tree/src/diff.rs`, `! test -e crates/flui-tree/src/state.rs`, `! test -e crates/flui-tree/src/traits/node.rs`, `! test -e crates/flui-tree/src/arity/storage.rs`, `! test -e crates/flui-tree/src/arity/arity_storage.rs`, `! test -e crates/flui-tree/src/arity/accessors.rs`, `grep 'remove_cascade_is_stack_safe_on_deep_chain' crates/flui-tree/src/traits/write.rs` exits 0 (PR #103 Codex P2 regression).

---

## 14. References

- `openspec/changes/core-0a-foundation-parity-to-flutter/proposal.md` — re-scoped proposal (post-cycle-3 reality).
- `openspec/changes/core-0a-foundation-parity-to-flutter/specs/*` — 8 domain specs with 74 RFC 2119 requirements.
- `docs/research/2026-05-22-flui-foundation-tree-audit.md` — 47-finding cycle-3 audit (Part III drift catalog at line 1647; Part IV combined priority order at line 1771; cycle-3 closure tables at lines 2200–2256).
- `docs/PORT.md` — port methodology + refusal triggers + per-crate ARCHITECTURE.md template (`:756+`) + Index (`:790+`).
- `docs/FOUNDATIONS.md` Part IV — target crate decomposition.
- `crates/flui-foundation/ARCHITECTURE.md` — existing templated doc (grafted 2026-05-19); amendment target for PR2.
- `scripts/port-check.sh` — 13-trigger refusal gate (#8–#13 installed PR #151).
- `openspec/config.yaml` — `require_tradeoffs: true`, `strict_tdd: true`, `test_command: cargo test --workspace`, `verify.test_command: just ci`, `protect_review_workload: true`.
- Predecessor envelopes in chain run `949e3e92`: `init.md` → `proposal.md` → `spec.md` → (this) `design.md`.
- Project lead mandate: *"должно быть как в .flutter по контракту но архитектурно лучше и эргономичнее используя функционал раста и его паттерны и нам breaking разрешен."*

---

*End of design. Chain proceeds to `tasks.md` (next sdd-plan step). If the supervisor wants to override any §4.6 verdict, the §4.2 depth-constant shape, the §4.5 ObserverList disposition, the §3 peer-review skip, the §5 PR ordering, or the §7 port-check.sh extension, this is the gate.*
