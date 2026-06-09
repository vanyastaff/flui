# Proposal — `core-0a-foundation-adversarial-reaudit`

| Field | Value |
|---|---|
| Change ID | `core-0a-foundation-adversarial-reaudit` |
| Phase | Core.0 (sub-change 0a — adversarial re-audit of cycle-3 foundation work) |
| Owner crates | `crates/flui-foundation`, `crates/flui-tree`, `crates/flui-macros` |
| Workflow | SDD (strict TDD per `openspec/config.yaml`) |
| Upstream deps | `core-0a-ratification-superseded-2026-05-25` (cycle-3 context; superseded as source of NEW findings) |
| Downstream gates | Core.0b (layer/semantics repair), Core.0c (C2/C3/C4+C6 contracts), Core.0d (D-block pipeline wiring) |
| Strict-TDD | **true** (RED → GREEN → REFACTOR mandatory) |
| Breaking changes allowed | **yes** — project lead standing mandate |
| Review budget per task | 400 changed lines (per `openspec/config.yaml rules.tasks.protect_review_workload`) |
| Exploration reference | `openspec/changes/core-0a-foundation-adversarial-reaudit/exploration.md` |
| skill_resolution | `paths-injected` (multi-agent, rust-ownership-system) |

---

## 1. Problem Frame

Per `openspec/config.yaml rules.proposal.require_problem_statement: true`.

### 1.1 The mandate: cycle-3 was done with weaker tools

Cycle 3 (PRs #102–#106 + Polish, 2026-05-22 → 2026-05-23) performed a substantive audit and hardening of `flui-foundation` and `flui-tree`: it deleted ~11,600 LOC of zombie surface, landed the iterative-cascade contract, consolidated depth constants, and closed 34 of the 47 cycle-3 findings. It was authoritative and correct *for its tooling level*.

The project lead's binding mandate for this cycle is:

> **"Cycle 3 was done with weaker tools; advanced SDD + Rust 1.95+ + cross-vendor advisor should find what cycle 3 missed."**

This change is an **adversarial re-examination** of cycle-3's closures and deferrals under:
- Advanced SDD method (10-dimension audit: D1 Soundness, D2 Concurrency, D3 Variance/Lifetime, D4 Edition 2024/1.95 idiom, D5 Flutter parity, D6 GPUI patterns, D7 bon adoption, D8 Diagnosticable derive, D9 Inline storage, D10 Test coverage)
- Rust 1.95 / edition 2024 standards (including `#[expect]`, `fetch_update`, `NonZeroU64::new` — idioms unavailable or unenforceable at cycle-3 toolchain)
- Cross-vendor advisory (Codex/OpenAI gpt-5-codex broadcast on the top-3 P0 findings)

This is **not a ratification**. The prior `core-0a-ratification-superseded-2026-05-25` proposal ratified cycle-3's *documented* decisions. This change challenges cycle-3's undocumented assumptions and the audit axes it did not traverse.

### 1.2 Evidence: the severity histogram

The exploration (`exploration.md`, 1,032 lines, 30 findings) produced:

| Severity | Count | Finding IDs |
|---|---|---|
| **P0** (UB / production crash class) | **3** | F2, F6, F19 |
| P1 (Flutter-parity contract / test gap) | 5 | F5, F11, F15, F17, F24 |
| P2 (observable divergence / hot-path regression) | 8 | F4, F10, F12, F13, F14, F21, F25, F30 |
| P3 (idiom / edition drift / doc gap) | 14 | F1, F3, F7, F8, F9, F16, F18, F20, F22, F23, F26, F27, F28, F29 |
| **Total** | **30** | — |

Three P0 findings — `NonZeroU64::new_unchecked(0)` UB, listener-panic aborting notify, and cycle-detection missing from the cascade walk — survive a Rust UB discipline that does not accept "unreachable in practice." All three were cycle-3 blind spots: F2's I-7 deferral missed the post-wrap state machine; F6's I-4 closure addressed allocator pressure, not listener-loop hazards; F19's PR #103 hardened deep-valid trees but never wrote the corrupted-cyclic test.

### 1.3 Evidence: three Codex cross-vendor verdicts

The parent harness re-broadcast the top-3 findings to Codex (OpenAI gpt-5-codex) after the explore-step child harness failed shell access. The verdicts are appended in `exploration.md` and materially change two of the three fix shapes:

| Finding | Codex verdict impact |
|---|---|
| **F2 — Key::new UB** | Fix shape CHANGED. `NonZeroU64::new(id).expect(...)` eliminates UB but introduces duplicate keys after catch_unwind+retry. The correct fix is the `fetch_update` sentinel pattern: counter=0 is the permanent-exhaustion sentinel, retries panic without mutation or duplicates. |
| **F6 — listener panic aborts notify** | Fix validated unchanged. `catch_unwind(AssertUnwindSafe(|| callback()))` + `tracing::error!` is the correct Rust-1.95 idiom. Mandates a regression test: "listener 2 fires after listener 1 panics." |
| **F19 — cascade cycle-detection missing** | Severity DISPUTED. Codex disputes P0 unless corrupted storage is attacker-reachable through deserialization/plugin/FFI/unsafe. Severity downgraded to **P1** (defense-in-depth, not actively exploitable through the public API). Fix shape CHANGED: prefer `HashSet<I>` for visited set (avoids O(N²) on large valid removals); add `try_remove() -> Result<Option<Node>, TreeError>` as the semantic-carrying API; have `remove()` call it and return `None` + `tracing::warn!` on cycle detection. |

This is GENUINE work, not box-ticking. Three P0 findings with two Codex-modified fix shapes cannot emerge from a ratification run.

### 1.4 Stakeholder impact

| Stakeholder | Impact if defects ship to Core.0b/0c/0d |
|---|---|
| **Every downstream crate** (`flui-scheduler`, `flui-interaction`, `flui-rendering`, `flui-layer`, `flui-semantics`, `flui-view`, `flui-animation`, `flui-app`, `flui-platform`) | F6 UB/crash class: a buggy user listener in any binding-based crate (`SchedulerBinding`, `WidgetsBinding`) can abort the entire notify loop, corrupting frame-pipeline state. F2 UB: `NonZeroU64::new_unchecked(0)` can be reached in test harnesses that catch panics. F5: removed listeners still fire, breaking change-handler cleanup contracts. |
| **Core.0b/0c/0d sub-changes** | F19 missing cycle guard + F24 Vec→SmallVec: cascade walk has no cycle-detection as Core.0b begins wiring real render/layer/semantics trees. F11 surprising `Default`: downstream derives on structs containing `ValueNotifier<T>` produce non-equal notifiers with equal values — hidden divergence under `==`. |
| **Every future widget author** | F15 missing Diagnosticable derive: every new widget hand-rolls 15-20 LOC of boilerplate. F12 shallow DiagnosticsProperty: devtools reports lose type discrimination. F1 vacuous unsafe: contributors writing similar code infer that `unsafe fn` means "has safety preconditions" when it does not. |
| **Audit cycle quality** | F9/F21/F22 edition drift: future audits find the same `#[allow]` vs `#[expect]` delta because the current codebase did not set the standard. |

Foundation defects compound multiplicatively. Every downstream crate that inherits `ChangeNotifier` inherits F5 and F6. Every crate that uses `Key::new` inherits F2. The blast radius of a post-Core.0b fix is proportional to the number of crates already built on the defective base.

### 1.5 Risk if not addressed

| # | Risk | Severity |
|---|---|---|
| RR1 | F2 UB (`NonZeroU64::new_unchecked(0)`) becomes permanently load-bearing in test harnesses that catch panics (tokio task boundaries, `#[test]` harnesses with `should_panic`, plugin shells). After Core.0b ships and binds Key generation to frame lifetimes, the reachability footprint grows. | High |
| RR2 | F6 listener-panic propagation: Core.0b/0c/0d introduce new BindingBase consumers (`RendererBinding`, `WidgetsBinding`, `SemanticsBinding`). A buggy listener in ANY of these can abort the entire notify chain, producing stale frame state. Risk compounds linearly with number of bindings wired. | High |
| RR3 | F19 cascade missing cycle-guard: Core.0b begins wiring render/layer trees on top of `TreeWrite`. If a slab-manipulation bug or a deserialization path introduces a cycle, OOM occurs without a tracing-level diagnostic. The Ancestors iterator already has cycle-bound (T-12); the cascade walk lacking it is a documented asymmetry that future auditors will independently rediscover. | Medium |
| RR4 | F5 remove-during-notify semantics: Core.0c builds the element tree's lifecycle management on ChangeNotifier. If the "removed listener still fires" behavior is relied upon by element-lifecycle disposal, the fix becomes a semantic breaking change at app scale rather than a controlled breaking change now. | Medium |
| RR5 | F11 `Default for ValueNotifier` + misleading `PartialEq`: as Core.0c derives equality on element-state structs containing `ValueNotifier<T>`, the `==`-but-different-notifier-identity divergence creates hidden bugs that are difficult to diagnose at the element-reconciliation layer. | Medium |
| RR6 | Cost of post-Core.0b fix: every fix to `ChangeNotifier`, `Key`, or `TreeWrite` after Core.0b/0c/0d ship requires a workspace-wide cascade-change plus a migration path for any downstream crates already wired. Fixing now, before any of those ship, has maximum leverage and minimum cost. | High |

---

## 2. Intent

Harden the foundation-layer crate pair (`flui-foundation` + `flui-tree` + `flui-macros`) against the 30 findings surfaced by adversarial re-audit, closing all findings that are cleanly fixable within the 400 LOC review budget, and recording structured defer triggers for the three findings where the fix shape requires design work beyond this change's scope.

Specific objectives:

1. **Eliminate P0 UB and crash-class bugs** — F2 (`Key::new` UB via `new_unchecked`), F6 (listener-panic propagation), F19 (cascade cycle OOM) using the Codex-validated fix shapes.
2. **Close Flutter-parity contracts** — F5 (remove-during-notify semantic), F11 (Default for ValueNotifier), completing cycle-3's incomplete I-4 and I-5 sweeps.
3. **Reduce foundation unsafe surface** — F1 (vacuous `unsafe fn`), F2 (`new_unchecked`), F29 (reinvented `debug_assert_valid!`), F23 (I-10 deferral closure).
4. **Install edition-2024 idiom baseline** — F9 (`#[allow]` → `#[expect]`), F21 (blanket allow → per-site expect), F22 (clippy::pedantic alignment), F28 (Identifier `Into<Index>` redundancy), F30 (`.get()` canonical path).
5. **Add Diagnosticable infrastructure** — F12 (typed `DiagnosticsProperty` variants), F15 (`#[derive(Diagnosticable)]` proc-macro in `flui-macros`), F27 (strip module path from `type_name`).
6. **Fill test coverage gaps** — F17 (BindingBase retry test), F18 (Id boundary test), F19 (cascade cycle regression test), F6 (listener-2-fires-after-1-panics regression test).
7. **Close residual P2/P3 technical debt** — F3 (UniqueKey overflow), F4 (BindingBase CAS), F7 (PhantomData variance), F8 (HRTB over-engineering), F14 (bon builder for Slot), F16 (SmallVec comment), F20 (check_disposed layout), F24 (Vec→SmallVec in cascade), F26 (println! in doctests).

### 2.1 What this change does NOT do

- Does **not** re-litigate the 34 cycle-3-closed findings (PRs #102–#106). Their closures stand.
- Does **not** introduce `MergedListenable` (F10 — design decision on composite `ListenerId` semantics deferred; see §3 triage table).
- Does **not** re-introduce `ObserverList` (F13 — the de-dup architectural note deferred to the flui-interaction audit cycle; see §3 triage table).
- Does **not** fix disposal order in cascade (F25 — complex two-stack post-order fix has breaking-change risk for engine consumers; see §3 triage table).
- Does **not** touch `flui-geometry`, `flui-platform`, `flui-types::physics`, `flui-scheduler`, or any crate outside the owner-crate set.
- Does **not** re-enable `flui-reactivity` or `flui-devtools`.
- Does **not** touch Core.0b/0c/0d scope (layer, element, render, D-block).

---

## 3. Triage table

For every finding F1..F30, one verdict is assigned. Verdicts determine inclusion in spec + design + tasks.

Legend:
- **FIX** = `fix-in-this-change` — clean fix shape, ≤400 LOC delta per task, strict-TDD-able
- **DEFER** = `defer-to-followup-with-trigger` — real finding; trigger condition recorded
- **ACCEPT** = `accept-no-action-with-rationale` — real finding; cycle-3 choice stands
- **REJECT** = finding is wrong

| ID | Severity | Dim | Title (abbreviated) | Verdict | Rationale / Trigger |
|---|---|---|---|---|---|
| F1 | P3 | D1 | `Id<T>::from_raw` vacuous `unsafe fn` | **FIX** | ~5 LOC; removing `unsafe` from a public fn never breaks callers; eliminates misleading API signal |
| F2 | P0 | D1 | `Key::new()` off-by-one + UB on post-wrap | **FIX** | Codex verdict adopted: `fetch_update` sentinel (counter=0 = permanent exhaustion); eliminates UB AND duplicate-key risk in one shape |
| F3 | P3 | D1 | `UniqueKey::new()` no overflow check | **FIX** | ~3 LOC; mirrors Key's guard; contract "each UniqueKey is different" must be enforced at the boundary |
| F4 | P2 | D2 | `BindingBase::instance()` steady-state Release store | **FIX** | ~6 LOC CAS swap; observable-semantic identical; removes per-frame cache-coherence pressure on all 5 production bindings |
| F5 | P1 | D2+D5 | Removed listener still fires during notify | **FIX** | Flutter parity contract (D5); fix shape: snapshot includes listener IDs, re-check `contains_key` before each callback; breaking change explicitly allowed |
| F6 | P0 | D1+D2+D5 | Listener panic aborts all remaining notifies | **FIX** | Codex verdict adopted (validated unchanged); `catch_unwind(AssertUnwindSafe(|| callback()))` + `tracing::error!`; regression test mandated: "listener-2 fires after listener-1 panics" |
| F7 | P3 | D3 | `Id<T>` `PhantomData<T>` covariant; should be invariant | **FIX** | 1 LOC (`PhantomData<fn() -> T>`); no caller impact for current `'static` markers; future-proofs parameterized markers |
| F8 | P3 | D3+D4 | HRTB `for<'a> FnMut(&'a Self::Node)` over-engineered | **FIX** | ~8 LOC across `TreeReadExt` + `TreeNavExt`; simpler bound, identical inference; relaxation never breaks callers |
| F9 | P2 | D4 | `#[allow(unsafe_code)]` → `#[expect]` sweep | **FIX** | <20 LOC; forces lint attribute removal when the unsafe is eliminated; edition-2024 mandatory idiom |
| F10 | P2 | D5 | Missing `MergedListenable` | **DEFER** | Design decision on composite `ListenerId` semantics unresolved; large LOC (~400+ new). **Trigger:** first in-workspace consumer requiring "subscribe one listener to N Listenables" (flui-animation `CompoundAnimation` or flui-interaction `GestureArena`) |
| F11 | P1 | D5 | `Default for ValueNotifier<T>` surprising + PartialEq misalignment | **FIX** | Remove `impl Default for ValueNotifier<T>`; cycle-3 I-5 closed `Default for Key/UniqueKey` on identical rationale but missed this; breaking change allowed |
| F12 | P2 | D5 | `DiagnosticsProperty` typed subclasses missing | **FIX** | ~300 LOC; typed variants preserve enum/flag/iterable semantics for devtools; additive if `DiagnosticsProperty::new` forwards to `Generic` variant |
| F13 | P2 | D5 | ObserverList de-dup semantic lost; no migration doc | **DEFER** | Code re-introduction is out of scope; architectural note must live in flui-interaction's ARCHITECTURE.md (wrong crate for this change). **Trigger:** flui-interaction hit-test enhancement reaches de-dup requirement OR `flui-devtools` re-enable requires ordered observer iteration |
| F14 | P2 | D7 | `bon` builder sweep for `Slot::with_siblings` + `Slot::new` | **FIX** | T-17 (cycle-3 deferred); `Slot::with_siblings` has 5 positional args with two indistinguishable `Option<I>` — the canonical `bon` builder case; `bon` already a workspace dep per constitution Part IV |
| F15 | P1 | D8 | `#[derive(Diagnosticable)]` macro missing | **FIX** | ~200-400 LOC proc-macro in `flui-macros`; saves ~15 LOC per downstream Diagnosticable impl × 10+ existing render objects; opt-in derive, additive |
| F16 | P3 | D9 | SmallVec retention rationale undocumented | **FIX** | ~3 LOC comment; documents `tinyvec` rejection (`ListenerCallback: !Default`) for future maintainers |
| F17 | P1 | D10 | Missing BindingBase retry-after-panic test | **FIX** | ~40 LOC test; symmetric to existing `init_panic_does_not_flip_initialized_flag`; required to cover `OnceLock::get_or_init` recovery semantic |
| F18 | P3 | D10 | No test for `Id` at `usize::MAX - 1` boundary | **FIX** | ~25 LOC test; boundary behavior documented via test |
| F19 | P0→**P1** (Codex) | D1+D10 | Cascade walk missing cycle-detection guard | **FIX** | Codex downgraded from P0 to P1 (not attacker-reachable through standard public API); fix shape changed: `HashSet<I>` visited set (avoids O(N²)); new `try_remove() -> Result<Option<Node>, TreeError>`; `remove()` calls `try_remove()`, returns `None` + `tracing::warn!` on cycle; regression test: cycle-creates-OOM-without-guard |
| F20 | P3 | D2+D4 | `check_disposed` `debug_assert!/tracing::warn!` misleading layout | **FIX** | ~10 LOC; `#[cfg(debug_assertions)]` / `#[cfg(not(debug_assertions))]` makes the dead-code path explicit |
| F21 | P2 | D4 | `flui-tree` blanket `#![allow]` should be per-site `#[expect]` | **FIX** | Per-fn `#[expect(clippy::too_many_lines, reason = "...")]` sweep across `flui-tree/src/`; removes blanket allowance; ~30 LOC |
| F22 | P3 | D4 | clippy::pedantic asymmetry between foundation and tree | **FIX** | ~5 LOC; add `clippy::pedantic` to `flui-foundation::lib.rs` lint stack to match `flui-tree`; enforces consistent audit baseline across the sibling pair |
| F23 | P3 | D4 | I-10 deferral rationale invalid; scheduler imports unused | **FIX** | Close I-10: (1) remove `Index, RawId` from `flui-scheduler/src/id.rs` unused imports; (2) downgrade `pub struct RawId` → `pub(crate)`, `pub type Index` → `pub(crate) type Index` in `flui-foundation`; ~10 LOC |
| F24 | P1 | D2 | `TreeWrite::remove` cascade uses `Vec` not `SmallVec` | **FIX** | Merged with F19 fix; both `Vec<I>` replaced with `SmallVec<[I; INLINE_TREE_DEPTH]>`; zero allocator pressure for typical subtrees |
| F25 | P2 | D5 | Cascade disposal order reversed vs Flutter left-to-right | **DEFER** | Two-stack post-order traversal is non-trivial; engine consumers may observe current right-to-left order; breaking change needs per-consumer audit. **Trigger:** first disposal-order-sensitive engine hook is added to `flui-rendering` or `flui-layer`, or Core.0b audit flags observable order divergence |
| F26 | P3 | D4+D5 | `println!` in doc-comment doctests (Constitution Principle 6) | **FIX** | Sweep `flui-foundation/src/` doc-comments; replace `println!` with `tracing::info!` or comment placeholder; ~20 LOC |
| F27 | P3 | D5 | `type_name` full module path in `to_diagnostics_node` | **FIX** | ~5 LOC; mirror `Id::fmt`'s `rsplit("::").next()` strip; devtools output becomes `RenderPadding` not `flui_rendering::objects::render_padding::RenderPadding`; breaking for string-match tests (breaking allowed) |
| F28 | P3 | D4 | `Identifier` trait redundant `Into<Index>` bound | **FIX** | ~8 LOC; remove `+ Into<Index>` from `Identifier` supertrait; callers that need `usize` use `.get()` canonical path |
| F29 | P3 | D4 | `debug_assert_valid!` reinvents `debug_assert!` | **FIX** | ~80 LOC reduction; replace all consumers with stdlib `debug_assert!`; delete the custom macros; no in-workspace external consumers |
| F30 | P2 | D4 | `TreeWriteNav::move_children`/`insert_child` use `I: Into<usize>` | **FIX** | ~6 LOC; replace `from.into()` with `from.get()`; drop the `I: Into<usize>` bound; closes the T-14 open half |

**Totals:** FIX = 27 | DEFER = 3 | ACCEPT = 0 | REJECT = 0

---

## 4. Scope

### 4.1 In scope

Every finding triaged as `fix-in-this-change` above (27 findings). Grouped by crate and theme:

| Area | Findings | Modality |
|---|---|---|
| `crates/flui-foundation/src/id.rs` | F1 (vacuous unsafe), F7 (PhantomData), F9 (#[expect]), F23 (I-10 closure) | Code edit |
| `crates/flui-foundation/src/key.rs` | F2 (fetch_update sentinel), F3 (UniqueKey overflow), F9 (#[expect]) | Code edit |
| `crates/flui-foundation/src/binding.rs` | F4 (CAS optimization), F17 (retry test) | Code + test edit |
| `crates/flui-foundation/src/notifier.rs` | F5 (remove-during-notify), F6 (catch_unwind), F11 (Default removal), F16 (comment), F20 (check_disposed), F26 (doctest println) | Code + test edit |
| `crates/flui-foundation/src/debug.rs` | F12 (typed property variants), F15 (Diagnosticable derive support), F27 (type_name strip) | Code edit |
| `crates/flui-foundation/src/assert.rs` | F29 (delete reinvented macros) | Code delete |
| `crates/flui-foundation/src/lib.rs` | F22 (clippy::pedantic), F26 (doctest println) | Code edit |
| `crates/flui-macros/src/lib.rs` | F15 (Diagnosticable derive proc-macro) | Code add |
| `crates/flui-tree/src/traits/write.rs` | F19+F24 (cycle-guard + SmallVec + try_remove), F30 (.get() canonical) | Code edit |
| `crates/flui-tree/src/traits/read.rs` | F8 (HRTB drop) | Code edit |
| `crates/flui-tree/src/traits/nav.rs` | F8 (HRTB drop) | Code edit |
| `crates/flui-tree/src/iter/slot.rs` | F14 (bon builder for Slot) | Code edit |
| `crates/flui-tree/src/lib.rs` | F21 (per-site #[expect]), F22 (clippy align) | Code edit |
| `crates/flui-tree/src/error.rs` (via F19) | F19 (`TreeError::CycleDetected` variant) | Code add |
| `crates/flui-foundation/src/id.rs` + tests | F18 (boundary test) | Test add |
| `crates/flui-scheduler/src/id.rs` | F23 (unused imports cleanup) | Code edit |

**Net LOC estimate:** ~2,500–3,600 LOC total churn.
- Net-additive (new code): ~600 LOC (Diagnosticable macro + typed property variants + tests)
- In-place tightening: ~2,000 LOC
- Net deletion (assert macros + SmallVec rewrites + Default removal): ~400 LOC

### 4.2 Out of scope (deferred with triggers)

| Finding | Trigger condition | Future change ID |
|---|---|---|
| **F10** — `MergedListenable` | First in-workspace consumer requiring multi-Listenable subscription (flui-animation `CompoundAnimation` or flui-interaction `GestureArena`) | `core-0a-merged-listenable` (sdd-new) |
| **F13** — ObserverList de-dup semantic documentation | flui-interaction hit-test enhancement reaches de-dup requirement OR flui-devtools re-enable requires ordered observer iteration | Recorded in `crates/flui-interaction/ARCHITECTURE.md` `## Outstanding refactors` at Core.0b authoring time |
| **F25** — Cascade disposal order (left-to-right parity) | First disposal-order-sensitive engine hook in `flui-rendering` or `flui-layer`, OR Core.0b audit explicitly flags observable disposal order divergence | `core-0b-cascade-disposal-order` (sdd-new) |

---

## 5. Affected areas

| Path | Change type | Notes |
|---|---|---|
| `crates/flui-foundation/src/id.rs` | EDIT | F1: `pub unsafe fn from_raw` → `pub fn from_raw`; F7: PhantomData variance; F9: #[expect]; F23: RawId/Index visibility |
| `crates/flui-foundation/src/key.rs` | EDIT | F2: `fetch_update` sentinel replaces `fetch_add + new_unchecked`; F3: UniqueKey overflow assert; F9: #[expect] |
| `crates/flui-foundation/src/binding.rs` | EDIT + TEST | F4: CAS instead of store; F17: retry-after-panic test |
| `crates/flui-foundation/src/notifier.rs` | EDIT + TEST | F5: pre-fire ID re-check; F6: catch_unwind per callback; F11: remove Default impl; F16: SmallVec comment; F20: cfg-gated check_disposed; F26: doctest cleanup |
| `crates/flui-foundation/src/debug.rs` | EDIT | F12: DiagnosticsPropertyKind enum + constructors; F27: type_name strip |
| `crates/flui-foundation/src/assert.rs` | EDIT | F29: delete custom debug_assert macros |
| `crates/flui-foundation/src/lib.rs` | EDIT | F22: clippy::pedantic; F26: doctest cleanup |
| `crates/flui-macros/src/lib.rs` | EDIT | F15: Diagnosticable derive expansion |
| `crates/flui-tree/src/traits/write.rs` | EDIT + TEST | F19: try_remove + HashSet visited + cycle guard; F24: SmallVec; F30: .get() |
| `crates/flui-tree/src/traits/read.rs` | EDIT | F8: drop for<'a> |
| `crates/flui-tree/src/traits/nav.rs` | EDIT | F8: drop for<'a> |
| `crates/flui-tree/src/iter/slot.rs` | EDIT | F14: #[bon::builder] on Slot::with_siblings and Slot::new |
| `crates/flui-tree/src/lib.rs` | EDIT | F21: per-site #[expect]; F22: clippy align |
| `crates/flui-tree/src/error.rs` | EDIT | F19: `TreeError::CycleDetected` variant |
| `crates/flui-tree/src/depth.rs` | (read-only) | INLINE_TREE_DEPTH constant used for SmallVec sizing (F19+F24) |
| `crates/flui-scheduler/src/id.rs` | EDIT | F23: remove unused `Index, RawId` imports |
| `openspec/changes/core-0a-foundation-adversarial-reaudit/spec.md` | NEW | Next SDD phase |
| `openspec/changes/core-0a-foundation-adversarial-reaudit/design.md` | NEW | Design phase (fix shapes, implementation decisions) |
| `openspec/changes/core-0a-foundation-adversarial-reaudit/tasks.md` | NEW | Tasks phase (≤400 LOC per task, strict TDD) |

**Downstream crates:** `flui-rendering`, `flui-layer`, `flui-semantics`, `flui-view`, `flui-app`, `flui-animation` (disabled), `flui-interaction`, `flui-scheduler` will pick up the API surface changes passively. F11 (`Default for ValueNotifier`) and F23 (visibility downgrade) require a downstream-callers grep before landing. Breaking changes are explicitly allowed per project lead mandate.

---

## 6. Risks

| # | Risk | Likelihood | Mitigation |
|---|---|---|---|
| RK1 | **F5 (remove-during-notify) behavioral breaking change.** flui-animation or flui-view may have tests or production code relying on "removed listener still fires" as a cleanup callback — the fire happens, the listener unregisters itself (idempotent). A sweep is needed before landing. | Medium | Design phase: grep flui-animation + flui-view for `remove_listener` inside a listener callback; document each callsite's assumption; adjust or add migration note. |
| RK2 | **F11 (Default removal) downstream breaks.** `#[derive(Default)]` on structs containing `ValueNotifier<T>` fails to compile after this change. | Medium | Design phase: grep workspace for `ValueNotifier` in struct fields + derived `Default`. `flui-animation` (8 files per cycle-3 audit) is the primary blast radius. Migration: replace derived `Default` with explicit `new(T::default())` constructors. Strict TDD: RED compile failure → GREEN migration → REFACTOR. |
| RK3 | **F23 (RawId/Index visibility downgrade) scheduler API break.** `flui-scheduler/src/id.rs` imports `RawId` / `Index`; downgrades make them `pub(crate)` in foundation. If any _external_ crate names these types, compilation fails. | Low | Design phase: `cargo search` + workspace grep confirms no external consumer; the cycle-3 I-10 deferral evidence (scheduler imports exist but `RawId`/`Index` are unused in scheduler impl bodies) supports that the downgrade is safe. |
| RK4 | **F2 (fetch_update sentinel) subtle duplicate-on-retry edge.** The `fetch_update` sentinel is the correct shape, but requires careful documentation of the counter-wrapping invariant so future maintainers don't "fix" it back to `fetch_add`. | Low | Inline SAFETY doc on the `fetch_update` call explains the sentinel. Regression test: `Key::new()` called after simulated counter=0 state panics, not produces a zero-valued key. |
| RK5 | **F15 (Diagnosticable proc-macro) scope creep.** Proc-macro in `flui-macros` can easily grow beyond the 400-LOC review budget for a single task. | Medium | Cap the macro at field-by-field `builder.add(stringify!(field), &self.field)` with a single `#[diagnostic(skip)]` attribute. More complex rendering (EnumProperty, FlagProperty in F12) is handled in `DiagnosticsPropertyKind`'s display impl, not in the macro. The macro task is bounded at ~200-300 LOC net-additive. |
| RK6 | **F19 (HashSet visited set) bound change.** Introducing `HashSet<I>` requires `I: Hash + Eq`. The `Identifier` supertrait currently bounds `I: Copy + Clone + Eq + PartialEq + Hash + Debug`. Hash is already in the bounds. Verify at design time. | Low | Pre-checked: `crates/flui-tree/src/lib.rs` line 1 — `pub use traits::Identifier` which requires `Hash + Eq`. Risk is low; confirm in design phase. |
| RK7 | **F29 (delete assert macros) prelude breakage.** The macros are in `flui-foundation::prelude`. Any external (out-of-workspace) crate using the prelude would fail. | Very low | This is a workspace-internal crate; no published crate version. Grep confirms zero workspace consumers outside `flui-foundation/src/` itself. |
| RK8 | **Review budget overflow.** 27 findings across ~3,000 LOC must chunk into ≤400-LOC tasks. Grouping by crate/theme (as in §5) enables clean chunking. However F6 (catch_unwind + test) + F5 (snapshot ID re-check) in the same notifier edit may approach the limit. | Medium | Design phase allocates tasks as follows: group 1 (Soundness: F2+F3+F1), group 2 (Notifier: F5+F6+F11+F16+F20), group 3 (Cascade: F19+F24+F30), group 4 (Idiom sweep: F4+F7+F8+F9+F21+F22+F23+F26+F28+F29), group 5 (Diagnostics: F12+F15+F27), group 6 (Tests: F17+F18), group 7 (Slot bon builder: F14). Each group is a single task; the largest (group 4 idiom sweep) is estimated at ~300 LOC and fits within budget. |

---

## 7. Rollback

- **Every task is independently TDD-gated.** Each task has a failing test commit before the fix commit. If a task introduces a regression, `git revert <task-merge-commit>` restores the pre-task baseline without touching other tasks.
- **No on-disk format changes.** No serialization wire format, no persisted file format, no on-disk migration is involved. Every change is compile-time or runtime behavior of Rust code.
- **F11 (Default removal) is the highest-impact break.** If the migration sweep is incomplete and downstream compilation fails, the task is rolled back and the missing migration sites are added to the task before re-landing.
- **F2 (fetch_update) is safe to roll back** — the old `fetch_add + new_unchecked` is behavior-equivalent for non-exhausted counters. The regression window between rollback and re-land is bounded by the test harness catching the `new_unchecked(0)` path.
- **F23 (visibility downgrade) is trivially rollable** — `pub(crate)` → `pub` in one line per symbol, no downstream behavior change.
- **Constitution and FOUNDATIONS.md are not amended** by this change. The locked contracts (C1–C9) are unaffected.

---

## 8. Success criteria

Every criterion is command-form or test-pass form. The `sdd-verify` step runs all commands and records exit codes.

| # | Criterion | Verification |
|---|---|---|
| SC1 | `just ci` exits 0 (`cargo fmt --all -- --check` + `cargo clippy --workspace -- -D warnings` + `cargo test --workspace`) | `just ci` |
| SC2 | No `NonZeroU64::new_unchecked` in `crates/flui-foundation/src/key.rs` | `! grep -rn "new_unchecked" crates/flui-foundation/src/key.rs` exits 0 |
| SC3 | `Key::new()` regression test: counter=0 panics without producing a zero-valued key (no UB) | `cargo test -p flui-foundation key_counter_exhaustion` exits 0 |
| SC4 | Listener-2-fires-after-listener-1-panics regression test passing | `cargo test -p flui-foundation listener_fires_after_panic` exits 0 |
| SC5 | Removed-during-notify listener does NOT fire (Flutter parity) regression test passing | `cargo test -p flui-foundation removed_listener_does_not_fire_during_notify` exits 0 |
| SC6 | `impl Default for ValueNotifier<T>` is absent | `! grep -n "impl.*Default.*ValueNotifier" crates/flui-foundation/src/notifier.rs` exits 0 |
| SC7 | Cascade cycle OOM regression test: cyclic tree does not OOM or hang | `cargo test -p flui-tree cascade_cycle_detection` exits 0 (with `TreeError::CycleDetected` result and tracing::warn! emission) |
| SC8 | `try_remove` API exists in `TreeWrite` trait | `grep -n "fn try_remove" crates/flui-tree/src/traits/write.rs` exits 0 |
| SC9 | No `#[allow(unsafe_code)]` at module level in `crates/flui-foundation/src/id.rs` or `key.rs` (replaced by `#[expect]`) | `! grep -n "^#!\[allow(unsafe_code" crates/flui-foundation/src/{id,key}.rs` exits 0 |
| SC10 | No `println!` in doc-comment examples in `crates/flui-foundation/src/` | `! grep -rn "println!" crates/flui-foundation/src/` exits 0 |
| SC11 | `debug_assert_valid!` / `debug_assert_range!` / `debug_assert_finite!` / `debug_assert_not_nan!` macros absent from `crates/flui-foundation/src/assert.rs` | `! grep -n "macro_rules! debug_assert_valid\|debug_assert_range\|debug_assert_finite\|debug_assert_not_nan" crates/flui-foundation/src/assert.rs` exits 0 |
| SC12 | `#[derive(Diagnosticable)]` macro exists and expands correctly on a test struct | `cargo test -p flui-macros diagnosticable_derive_basic` exits 0 |
| SC13 | `DiagnosticsPropertyKind` enum exists with at least `Generic`, `Enum`, `Flag`, `Iterable`, `OptionalRef`, `Stack` variants | `grep -n "DiagnosticsPropertyKind" crates/flui-foundation/src/debug.rs` exits 0 |
| SC14 | `Slot::with_siblings` has `#[bon::builder]` annotation | `grep -n "bon::builder" crates/flui-tree/src/iter/slot.rs` exits 0 |
| SC15 | BindingBase retry-after-panic test passing | `cargo test -p flui-foundation instance_retries_after_panic` exits 0 |
| SC16 | `Id` boundary test at `usize::MAX` passing | `cargo test -p flui-foundation id_at_usize_max` exits 0 |
| SC17 | No `for<'a> FnMut(&'a Self::Node)` HRTB in `TreeReadExt` or `TreeNavExt` | `! grep -n "for<'a> FnMut" crates/flui-tree/src/traits/{read,nav}.rs` exits 0 |
| SC18 | `crates/flui-foundation/src/id.rs::from_raw` is NOT `unsafe fn` | `! grep -n "unsafe fn from_raw" crates/flui-foundation/src/id.rs` exits 0 |
| SC19 | `TreeWrite::remove` cascade uses `SmallVec` not `Vec` | `! grep -n "Vec<I>" crates/flui-tree/src/traits/write.rs` exits 0 |
| SC20 | `PhantomData<fn() -> T>` in `Id<T>` struct definition | `grep -n "PhantomData<fn() -> T>" crates/flui-foundation/src/id.rs` exits 0 |
| SC21 | `Identifier` trait does NOT have `Into<Index>` supertrait | `! grep -n "Into<Index>" crates/flui-foundation/src/id.rs` exits 0 (or only in impl blocks, not trait definition) |
| SC22 | Review budget: no individual task in `tasks.md` exceeds 400 changed lines | Per-task `git diff --shortstat` check in tasks.md verify step |
| SC23 | `bash scripts/port-check.sh -v` exits 0 (all refusal triggers clean) | `bash scripts/port-check.sh -v` |

A change-merge is gated on **all 23** criteria passing.

---

## 9. Notes for spec / design / tasks phases

### 9.1 Design phase priorities

The design phase must:

1. **Adopt Codex-modified fix shapes** for F2 and F19 (see §3 triage table and `exploration.md` cross-vendor verdicts section). The design phase MUST NOT revert to the exploration.md original fix shapes for these two findings.
2. **Allocate tasks by theme group** per RK8 mitigation:
   - Task 1 — Soundness cluster (F2, F3, F1 — id.rs + key.rs)
   - Task 2 — Notifier cluster (F5, F6, F11, F16, F20 — notifier.rs)
   - Task 3 — Cascade cluster (F19, F24, F30, F8 — write.rs + read.rs + nav.rs + error.rs)
   - Task 4 — Edition-2024 idiom sweep (F4, F7, F9, F21, F22, F23, F26, F28, F29)
   - Task 5 — Diagnostics cluster (F12, F15, F27 — debug.rs + flui-macros)
   - Task 6 — Test gap cluster (F17, F18)
   - Task 7 — Slot bon builder (F14)
3. **Confirm F19 `I: Hash + Eq` bound** (see RK6) before committing to `HashSet<I>` visited set.
4. **Produce downstream migration sweep** for F11 and F5 before those tasks land (see RK1, RK2).
5. **Write `try_remove` signature** that preserves the existing `remove() -> Option<Node>` public contract while adding the `try_remove() -> Result<Option<Node>, TreeError>` semantic-carrying path (Codex F19 recommendation).

### 9.2 Spec phase acceptance criteria

Per `openspec/config.yaml rules.spec.require_acceptance_criteria: true`, each acceptance criterion must map 1:1 to one or more of SC1..SC23 above, or be a new command-form criterion not listed here.

### 9.3 Strict-TDD discipline

Per `openspec/config.yaml rules.apply.strict_tdd: true`:
- Every code-touching task must have a failing-test commit before the fix commit
- The test must fail against the pre-fix source and pass after the fix
- Exception: doc-only sub-findings within a mixed task (e.g. F16 in the notifier cluster) do not require a RED test; they are bundled with the cluster's primary RED test

### 9.4 verify gate

`just ci` is the verify gate (per `openspec/config.yaml rules.verify.test_command: "just ci"`).

---

## 10. References

- `openspec/changes/core-0a-foundation-adversarial-reaudit/exploration.md` — 30 findings + severity histogram + Codex verdicts + cross-vendor impact section
- `openspec/changes/core-0a-ratification-superseded-2026-05-25/proposal.md` — cycle-3 ratification (superseded; provides context on what cycle-3 definitively closed)
- `docs/FOUNDATIONS.md` Part III–IV — locked contracts (C1–C9) and target crate graph
- `docs/ROADMAP.md` §Core.0 — phase definition and exit criteria
- `docs/PORT.md` — refusal triggers #1–#11; per-crate ARCHITECTURE.md template
- `STRATEGY.md` — three port rules ("behavior loyal, structure Rust-native"; "sync hot path")
- `openspec/config.yaml` — SDD rules (strict_tdd, require_problem_statement, protect_review_workload, test/verify commands)
- `crates/flui-foundation/src/` — audit target (5,915 LOC, 13 files)
- `crates/flui-tree/src/` — audit target (6,576 LOC, 16 files)
- `crates/flui-macros/src/lib.rs` — Diagnosticable derive target
- `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart` — F5/F6 parity reference
- `Rustonomicon §3.2` — UB basis for F2
- Project lead mandate (binding): *"cycle 3 was done with weaker tools; advanced SDD + Rust 1.95+ + cross-vendor advisor should find what cycle 3 missed"*; *"breaking разрешен"* (breaking changes allowed)

---

*End of proposal. Chain proceeds to `spec.md`. Cross-vendor verdicts on F2 and F19 MUST be integrated into spec acceptance criteria — do not use the exploration.md original fix shapes for these two findings.*
