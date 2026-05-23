---
title: "Architecture Correction Plan — FLUI ~236k LOC remediation"
date: 2026-05-22
status: research
research_type: foundations-input (architecture-correction, prioritized defect inventory, systemic-pattern synthesis)
scope: read-only audit; changes nothing; one of three parallel FOUNDATIONS inputs
crates_analyzed: 21 (15 active, 6 disabled)
synthesizes:
  - docs/research/2026-05-22-flutter-flui-gap-matrix.md
  - docs/research/2026-05-22-port-phasing-dependency-order.md
  - docs/research/2026-05-22-architectural-contracts.md
  - docs/research/2026-05-22-rust-ui-ecosystem-lessons.md
  - docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md
  - docs/research/2026-05-20-flui-painting-alloc-audit.md
  - docs/research/2026-05-21-flui-interaction-audit-draft.md
  - docs/research/2026-05-21-flui-interaction-scheduler-audit.md
  - docs/research/2026-05-21-flui-scheduler-audit-draft.md
  - docs/research/2026-05-21-view-tree-foundation-audit.md
  - docs/research/2026-05-22-flui-foundation-tree-audit.md
  - docs/research/2026-05-22-flui-layer-semantics-audit.md
  - docs/research/2026-05-22-flui-painting-view-audit.md
  - docs/research/2026-05-22-flui-rendering-engine-audit.md
  - docs/research/2026-05-22-cycle4-wave2-design.md
book_grounding:
  - "A Philosophy of Software Design — Ousterhout (complexity symptoms, deep/shallow modules, define-errors-out-of-existence, tactical tornado, temporal decomposition, information leakage, pass-through methods)"
  - "Rust for Rustaceans — Gjengset (sealed traits, typestate, split borrows)"
  - "Programming Rust 2nd ed — Blandy/Orendorff/Tisdale (enums in memory, phantom data)"
  - "The Rust Performance Book (allocator pressure, enum size)"
  - "STRATEGY.md — behavior loyal, structure Rust-native"
authors:
  - System Architect (consultant, via Claude Opus 4.7)
feeds: FOUNDATIONS decision doc (synthesized with tech-adoption matrix + crate-decomposition redesign)
---

# Architecture Correction Plan

> **Mandate.** Consolidate *every* known architecture defect and structural weakness in the current ~236k LOC FLUI workspace into one prioritized correction plan. Not a transcription of the audits — a synthesis. The two known-fatal defects (empty-body constraint propagation in `flui-rendering`; index-not-key reconciliation in `flui-view`) are the *floor* of this analysis, not the ceiling.
>
> **This document changes no code.** It is one of three parallel FOUNDATIONS inputs; where it depends on a sibling's conclusion (tech-adoption, crate-decomposition) it states the assumption inline.

---

## 0. Intro — what kind of problem this is

FLUI's render machine (`build → layout → paint → composite`) is 60–95% built; the user-facing widget/Material/Cupertino layer is ≈0%. The roadmap's plan is to construct ~480k LOC of widget catalog on top of that machine. **The machine is the foundation that catalog stands on, and the machine has structural defects.** This document inventories them.

The defects are not a random scatter of bugs. Read across all fourteen audits, they cluster into **eight systemic patterns** — the same *class* of mistake repeated across crates. This is the central finding, and it is diagnostically important: per Ousterhout (*A Philosophy of Software Design*, Ch. 19, "tactical programming" / the "tactical tornado"), a defect that recurs is not a defect — it is a *missing rule*. You do not fix a tactical tornado by fixing its commits one at a time; you fix it by installing the rule that would have refused the pattern. FLUI already has the mechanism for this: `docs/PORT.md`'s **refusal triggers**, of which there are seven, four of them forward-looking lints. The systemic patterns below each propose one new refusal trigger that kills the whole class.

A second framing matters for prioritization. Ousterhout names three symptoms of accumulated complexity: **change amplification** (one conceptual change touches many places), **cognitive load** (how much a developer must hold in their head), and **unknown unknowns** (the code that *looks* done but silently isn't). FLUI's most dangerous defects are overwhelmingly the third kind — and the third kind is *exactly* the kind that compounds when you build on top of it, because the builder cannot see the hole. The P0 tier below is defined as: **the unknown-unknowns under load.** A widget author building `ListView` on top of a reconciler that silently loses state on reorder will not discover the defect — their users will, in production, six months later.

A note on timing. The Mythos cycle is in flight and *has been closing defects while this document was being written.* Cycle-4 Wave-5 (PR #117, landed) deleted the three loud `unimplemented!()` macros in `flui-rendering`; the cycle-5 audit's V-3 (inherited registry) is now wired at `build_owner.rs:651`. This document was verified against HEAD source, not against the audit snapshots — where an audit finding is already closed, it is marked so. The Mythos-coverage delta in §6 is the authoritative covered-vs-new ledger.

**The one-line verdict, up front:** the render *machine* is structurally sound — typestate pipeline, lock-free `AtomicRenderFlags`, arity system, Slab+ID-offset are gold-standard and must not be touched. But three *phases* of that machine are stubbed-but-called, the *element reconciler* is the wrong algorithm, and ~17–58% of the foundation crates is written-but-uncalled scaffolding that inflates cognitive load without doing work. The foundation is **sound enough to build on after the P0 tier is closed** — and not before.

---

## 1. The systemic patterns (the most important section)

Eight patterns. Each is the *same class of mistake* appearing in 3+ places. For each: the named complexity symptom (Ousterhout), the instances, the one structural rule that kills the class, and — where the rule is enforceable — a proposed new `docs/PORT.md` refusal trigger.

### SP-1 — Stubbed-but-called: production methods that no-op or panic on the hot path

**The pattern.** A method sits in a production call path. Its body is empty, a `tracing::warn!`, or (until recently) `unimplemented!()`. Callers receive `Ok(())` or `()` and proceed as if work happened. **None of it happened.**

This is Ousterhout's **"unknown unknowns"** in its purest form — the worst category of complexity, because the defect is invisible at the call site. It is *worse than a panic*: a panic forces a fix; a silent no-op ships. The cycle-4 audit said it directly about `run_compositing`: *"Worse than R-1 because the panic is loud and forces a fix; this is silent and lets the bug remain hidden."*

**Instances (verified against HEAD source, 2026-05-22):**

| Site | State at HEAD | What does not happen |
|---|---|---|
| `flui-rendering/src/pipeline/owner.rs` `run_compositing` (~line 922) | no-op: sorts dirty list, logs `"compositing-bits update is a no-op until..."`, clears list, returns `Ok(())` | Compositing-bits update — Flutter's `flushCompositingBits`. Layer-tree compositing flags never set. |
| `flui-rendering/src/pipeline/owner.rs` `layout_node_with_children` (~line 855) | walks the tree checking `needs_layout()`, recurses — but **never calls `RenderEntry::layout(constraints)`**. Per-node layout (`perform_layout_raw`) happens *nowhere in production*. The only `entry.layout()` callsite is a `#[test]`. | **The entire layout phase.** Constraints are not propagated; `perform_layout` is not invoked; sizes are not computed. (Cycle-4 Wave-5 deleted the empty-body `propagate_constraints_to_child`/`sync_child_size_to_parent` stubs — *subtractively*, leaving the hole.) |
| `flui-rendering/src/pipeline/owner.rs` `run_semantics` (~line 1376) | post-PR#117: returns `Ok(())` with a `tracing::warn!` (was `unimplemented!()`) | Semantics tree construction. |
| `flui-engine` `WgpuPainter::clip_path` (`painter.rs:3592`) | silent no-op, `tracing::trace!` | Path clipping via the painter-direct route. (Layer-route `ClipPathLayer` works — two routes, one silently fails.) |
| `flui-engine` `Backend::render_backdrop_filter` (`backend.rs:805`) | fallback: dispatches child with no filter | Backdrop blur via the DisplayList-command route. (Layer route works.) |
| `flui-rendering` `RenderTree::set_owner` (`storage/tree.rs:114`) | stores the owner ref, docstring promises "attach all existing nodes" | attach/detach lifecycle on existing nodes. **Doc actively lies.** |
| `flui-interaction` `MouseTracker::update_all_devices` (`mouse_tracker.rs:357`) | no-op, `tracing::trace!` | Re-hit-test on layout change. Hover-over-moving-UI is broken. |
| `flui-interaction` `FocusManager::focus_next` / `focus_previous` (`focus.rs:270`) | `tracing::warn!("not yet implemented")` | Tab navigation. The fully-working `FocusScopeNode::focus_next_in_scope` exists, unreachable. |
| `flui-semantics` `SemanticsService::send_event` (`binding.rs:407`) | `tracing::debug!` + `// TODO` | The only platform-a11y routing path for events. |
| `flui-rendering` `run_paint` dirty-list handling (`owner.rs:983`) | clears `needs_paint` flags in a loop separate from the paint walk; paints only via `root_id` descent | Nodes-needing-paint not reachable from `root_id` get their flag cleared **without being painted**. |
| `flui-engine` `offscreen.rs::PipelineManager::get_or_create_pipeline` | body-less zombie wrapper | nothing. |

**Why this is the #1 pattern.** Eleven instances. It spans `flui-rendering`, `flui-engine`, `flui-interaction`, `flui-semantics` — every render-stack crate. And critically: **two of these (`layout_node_with_children`, `run_compositing`) are in the layout/composite phases that the entire widget catalog depends on every frame.** A widget catalog built today would produce correct *build* output and then nothing would lay out. The audits' own framing — "the render *machine* is sound; specific *phases* are stubbed" — is the precise diagnosis.

**The structural rule that kills the class.** A method that is reachable from a production entry point may not have a body that is `{}`, a bare `tracing::warn!`/`tracing::trace!` followed by `return`/end, or `unimplemented!()`/`todo!()`. If a phase is genuinely not ready, it must either (a) be removed from the call graph entirely (the typestate pipeline can omit a phase), or (b) return a typed `Err(RenderError::PhaseNotImplemented { phase })` that the caller is *forced by the type system to handle*. The distinction Ousterhout draws is "define errors out of existence" vs. "report errors honestly" — a silent no-op does *neither*; it pretends the error does not exist while the error is the entire behavior.

**Proposed refusal trigger #8 (`docs/PORT.md`):**

> **8. A production-reachable `fn` whose body is empty, a lone `tracing::{warn,trace,debug}!` + return, or `unimplemented!`/`todo!` — while the function name asserts an effect.**
> **Why:** stubbed-but-called methods are unknown-unknowns — the defect is invisible at the call site and ships silently. A phase that is not ready must be *absent from the call graph* (typestate omits it) or return a typed `Err` the caller must handle.
> **Detection:** `port-check.sh` greps `fn` bodies in non-`#[test]` modules of `flui-rendering`/`flui-engine`/`flui-interaction`/`flui-semantics` for the three shapes; cross-references against a hand-maintained `// STUB-OK: <reason>` allowlist. A `// STUB-OK` marker without a tracking issue is itself a violation.
> **Forward + retroactive:** unlike triggers 4/5/7, this one has *current production violations* (the eleven above) — it is enforced retroactively as the P0 gate, then guards against regression.

---

### SP-2 — Written-but-uncalled: the "correct" implementation exists, production uses the wrong one

**The pattern.** Two implementations of the same responsibility coexist. One is correct (Flutter-faithful, complete, tested). The other is a stub or a wrong-algorithm shortcut. **Production calls the wrong one. The correct one has zero production callers.**

This is the inverse of SP-1 and arguably more insidious: SP-1 has *nothing*; SP-2 has *the right thing, sitting unused*. The cost is double — the correct code is dead weight (cognitive load: a reader must determine which of two is live), and the wrong code is a live defect. Ousterhout's **information leakage** applies: the knowledge "use reconciler B, not A" lives nowhere in the type system; it is a fact a developer must *already know*.

**Instances:**

| Responsibility | The correct, uncalled impl | The wrong, called impl |
|---|---|---|
| **Variable-arity child reconciliation** (THE fatal one) | `flui-view/src/tree/reconciliation.rs::reconcile_children` — 325 LOC, full Flutter keyed O(N) linear algorithm (match-start / match-end / keyed-middle map / cleanup). Verified: production callers = **0** (only `lib.rs` re-export + the file's own tests). | `flui-view/src/element/child_storage.rs::VariableChildStorage::update_with_views` — 21-LOC `for (i, view) in views.iter().enumerate()` positional loop. In-code comment: `// TODO: In a full implementation, this would use keys for reordering`. This is what `ElementCore` calls. |
| Keyed reconciliation, even within B | `reconciliation.rs` *also* has a `ReconcileAction` enum (Update/Create/Remove/Move) for the canonical "return intentions then apply" shape — `#[allow(dead_code)]`, never constructed. | `reconcile_children` itself returns `Vec<ElementId>` directly, bypassing its own intention enum. |
| Tab-order traversal | `flui-interaction` `FocusScopeNode::focus_next_in_scope` (`focus_scope.rs:663`) — fully implemented, with `ReadingOrderPolicy`. | `FocusManager::focus_next` (`focus.rs:270`) — `tracing::warn!` stub. Two parallel `FocusManager`s; `global()` returns the flat-state one with the stub. |
| Tessellation | `flui-engine/src/wgpu/tessellator.rs` — 1322 LOC, the GPU-correct Lyon pipeline, production. | `flui-painting/src/tessellation.rs` — 537 LOC, `default`-feature-ON, **zero production consumers** (tests + one example). A *second* Lyon dep pulled into a Foundation crate. (Here the called one is correct; the *uncalled* one is the waste — same pattern, mirror polarity.) |
| Text layout | `Canvas::draw_text → DrawCommand::DrawText → engine` is the live text path. | `flui-painting` `TextPainter` (751 LOC) + `TextLayout` (cosmic-text, 457 LOC) + a `fallback::TextLayout` (257 LOC) — *all* test-only. The fallback exists to satisfy `TextPainter`; `TextPainter` exists to satisfy tests. |

**Why this is pattern #2.** It contains the single most dangerous defect in the workspace (see §3, D-2). And it is a *recognizable failure mode*: in every case, someone wrote the correct thing, then — to make the build green or a demo work — wired a shortcut, and never came back. Ousterhout's "tactical tornado" by name. The audits caught five instances; there are likely more in the disabled crates.

**The structural rule.** When two implementations of one responsibility exist, exactly one is canonical and the other must be *deleted in the same change*, not left "for reference." If the wrong one is load-bearing and the right one is incomplete, the change is "finish the right one and migrate" — never "leave both." This is the **[[no-quick-wins]]** discipline already in `CLAUDE.md`, stated as a structural rule rather than a work-ethic exhortation: *a parallel implementation is a defect even if both compile.*

**Proposed refusal trigger #9:**

> **9. Two functions/types in the workspace that implement the same responsibility, where one has zero production callers.**
> **Why:** a written-but-uncalled "correct" implementation means production runs the *other* one — and the knowledge of which is live exists nowhere in the type system. It is a tactical-tornado fingerprint.
> **Detection:** not fully mechanizable — enforced at review. The reviewer asks, for any new `fn`/`type` that resembles an existing one: "does the old one still have callers?" The `port-check.sh` half: a zero-production-caller `pub fn` whose name collides (case-insensitive stem) with another `pub fn` in the same crate is flagged for manual confirmation.
> **Retroactive:** the five instances above are P0/P1 work.

---

### SP-3 — Parallel types across crate boundaries: same concept, same name, two definitions, no bridge

**The pattern.** A single concept is defined twice in two crates, often under the *same name*, with no `impl`/`From`/sub-trait relating them. Consumers downstream must convert at the seam, or — worse — a `use ...::prelude::*` glob makes the name ambiguous.

This is Ousterhout's **change amplification** at crate scale: a conceptual change to "what a hit-test result carries" must be made in two places, kept in sync by hand. It is also pure **cognitive load** — a reader importing both preludes hits an ambiguity the compiler reports but cannot resolve. Rust's module system makes this *easy to do by accident* (every crate has its own namespace) and the audits found it everywhere.

**Instances (the audits caught these; cycle-4 Wave-2 design already addresses the rendering↔interaction cluster):**

| Concept | Definition A | Definition B | Status |
|---|---|---|---|
| `HitTestResult` | `flui-rendering/src/hit_testing/result.rs` (path + transform stack) | `flui-interaction/src/routing/hit_test.rs` (route entries) | cycle-4 Wave-2 design picks interaction as canonical, deletes rendering's. **Queued, not landed.** |
| `MouseTrackerAnnotation` | `flui-rendering` — a *trait* | `flui-interaction` — a *struct* with callback fields | cycle-4 Wave-2: interaction wins. Queued. |
| `MouseTracker` | `flui-rendering/src/input/mouse_tracker.rs` | `flui-interaction/src/mouse_tracker.rs` | cycle-4 Wave-2: delete rendering's whole `input/` module. Queued. |
| `RenderError` | `flui-rendering/src/error.rs` (pipeline errors) | `flui-engine/src/error.rs` (GPU errors) — **same name**, both `pub`, both `#[non_exhaustive]`, both alias `RenderResult<T>` | cycle-4 audit R-10: rename engine's → `EngineError`. **Not landed** (verified: `flui-engine/src/error.rs` still `pub enum RenderError`). |
| `ParentData` | `flui-rendering` (storage trait, `Any`-downcast) | `flui-view` (marker trait) | cycle-4 R-11 prefactor: `flui-view`'s renamed → `ParentDataConfig` in PR #84. **Closed.** |
| `PointerEvent` | `flui-painting/src/display_list/hit_region.rs` (minimal, for dead hit-regions) | `flui-interaction/src/pointer/event.rs` (W3C `ui-events`) | cycle-5 audit P-2: delete painting's whole `hit_region` module. Queued. |
| `ViewKey` | `flui-foundation/src/key.rs` (4 prod impls) | `flui-view/src/view/view.rs` (0 impls, empty) | view-tree-foundation audit: delete view-local. **Closed** (PR #84). |
| `IndexedSlot` | `flui-tree/src/iter/slot.rs` (`<I: Identifier>`) | `flui-view/src/element/slot.rs` (`<T>`) | view-tree-foundation audit. **Partially resolved** — flui-view re-exports `flui-tree`'s as `ElementSlot` per cycle-3. |
| `TargetPlatform` | `flui-foundation/src/platform.rs` (`Unknown` variant) | `flui-types/src/platform/target_platform.rs` (`Fuchsia` variant) | view-tree-foundation audit: keep `flui-types`. Status unverified. |
| `Color` / `ColorScheme` | `flui-types/src/styling/color.rs` (canonical, packed) | `flui-app/src/theme/colors.rs` (parallel, `f32` channels, 0 consumers) | cycle-5 V-25: delete `flui-app`'s. Queued. |
| Two error types in one crate | `flui-foundation` `FluiError` (assert.rs) + `FoundationError` (error.rs) — both 0 consumers | — | view-tree-foundation audit: delete `FluiError`. Status unverified. |

**Why this is pattern #3.** Eleven concept-pairs. The audits have *been finding this all along* — cycle-2 fixed `flui_types::Alignment`, cycle-4 fixed `ParentData`, cycle-4 Wave-2 fixes the hit-test trio. It keeps recurring because nothing *prevents* it. The fix-rate is roughly one pair per cycle; new pairs appear as fast.

**The structural rule.** A concept has exactly one canonical home in the DAG — the lowest crate that needs it. A second crate that needs the concept *imports* it; it does not redefine it. If a name appears in two `pub` surfaces, one of them is wrong. The corollary, which the audits keep re-deriving per-pair: the canonical home is usually the *consumer-shaped* crate (`HitTestResult` → `flui-interaction`, because Flutter's lives in `gestures/`), or the *lowest common dependency* (`flui-foundation` for IDs).

**Proposed refusal trigger #10:**

> **10. A public type or trait name that is defined in two `flui-*` crates without one being a re-export of the other.**
> **Why:** parallel cross-crate types are change-amplification (sync by hand) and a `prelude` glob ambiguity. The fix-rate has matched the appearance-rate for four Mythos cycles; only a trigger stops the cycle.
> **Detection:** `port-check.sh` collects every `pub struct`/`pub enum`/`pub trait` identifier across all `flui-*` crates and flags any identifier defined (not re-exported) more than once.
> **Retroactive:** ~7 unresolved pairs remain (rendering/engine `RenderError`, the hit-test trio, painting `PointerEvent`, `Color`, `FluiError`).

---

### SP-4 — Speculative scaffolding: large abstraction surfaces with zero production consumers

**The pattern.** A crate ships a sophisticated, well-documented abstraction — a trait family, a typestate machine, a generic storage layer — built *ahead* of any consumer. Months pass. No consumer materializes. The abstraction is now dead weight: it inflates the public API (every change is breaking), inflates compile time, and — most expensively — inflates **cognitive load**: a reader of the crate cannot tell the load-bearing 40% from the speculative 60%.

This is Ousterhout's central thesis inverted. He argues for **deep modules** — simple interface, much behind it. Speculative scaffolding is the opposite: a *large interface* with *nothing* behind it (no consumers exercising it). It is also a specific anti-pattern he names — designing for a *guessed* future requirement rather than a known one.

**The crucial distinction this plan must honor.** The user's memory `[[flui-tree-unified-interface-intent]]` and the view-tree-foundation audit's own "Post-audit correction" both state: `flui-tree`'s zero-consumer surface is **deliberate unified-tree infrastructure** — Flutter has four bespoke trees, `flui-tree` is the one-API consolidation, and zero-consumer = *migration gap*, not *deletion signal*. **This plan accepts that ruling.** The correction for SP-4 is therefore *not* "delete" — it is "make the speculation honest": feature-gate it behind `unstable-*` flags so it is out of the default compile and default cognitive surface until a consumer migrates onto it, OR finish the migration. The cycle-3 audit independently reached the same recommendation ("distinguish 'delete' from 'keep but feature-gate behind `unstable-devtools`'").

**Instances (magnitudes, from the audits):**

| Crate | Speculative surface | LOC / % of crate | Audit verdict |
|---|---|---|---|
| `flui-tree` | `visitor/` (2550), `diff.rs` (1234), `iter/cursor.rs` (1057), `iter/path.rs` (1150), `arity/{storage,arity_storage,accessors}` (3051), `state.rs` Mountable/Unmountable typestate (616), `traits/node.rs` (305) | **~10,600 LOC ≈ 58%** | foundation-tree audit: feature-gate (migration gap, per memory). Not "delete." |
| `flui-view` | `animated.rs`+`AnimationBehavior` (424), `parent_data.rs` (479), `error.rs` ErrorView (333), `root.rs` RootRenderView (577), notification parallel-tree (152) | **~1,965 LOC ≈ 14%** (≈58% if production-defined-but-test-only counted) | cycle-5: feature-gate `animated-views`/`parent-data-views`/`error-view`. |
| `flui-painting` | `tessellation.rs` (537), `text_painter/` (751), `text_layout/fallback.rs` (257), `canvas/sugar/` (720), `display_list/hit_region.rs` (101) | **~2,620 LOC ≈ 31%** | cycle-5: delete tessellation + hit_region; feature-gate sugar; feature-gate text-painter. |
| `flui-rendering` | `delegates/` 5 modules (~1800), `constraints/scroll_metrics.rs` (452), `ScrollableViewportOffset` listener API (~50) | **~2,300 LOC** | cycle-4 R-16/R-18/R-19: feature-gate `experimental-delegates`. |
| `flui-scheduler` | `typestate.rs` TypestateTicker (392), `Handle<M>` (120), 4 ZST priority types + `PriorityLevel` (145), `prelude_advanced` (14 exports), `VsyncDrivenScheduler` (134), 3 extension traits (150), `arc_instance()` parallel singleton (20) | **~1,500 LOC ≈ 17%** | scheduler audit: delete the pure-pedagogy typestate; the rest feature-gate or delete. |
| `flui-interaction` | `typestate.rs` (232), `one_sequence.rs`+`primary_pointer.rs` traits (823), `testing/` un-gated in release (1099) | **~2,150 LOC** | interaction audit: delete typestate; *migrate* recognizers onto the base traits (no-quick-wins) or delete; gate `testing/`. |
| `flui-foundation` | `ObserverList` (271), `FoundationError`+`FluiError` (335+), `MergedListenable`+`HashedObserverList` | **~900 LOC** | foundation-tree audit I-1/I-2: delete (these are *not* unified-tree infra — genuine YAGNI). |

**Why this is pattern #4.** It is the single largest *quantity* of defect — well over 20,000 LOC across seven crates. It does not crash anything. But it is the dominant driver of the "is this crate done?" confusion: the gap-matrix rates `flui-tree` as "High coverage" and `flui-rendering`'s machine as ~90% — both true by LOC, both misleading, because the LOC includes scaffolding nobody runs. A widget-catalog author reading `flui-tree` to learn the tree API faces a 58%-noise signal.

**The structural rule.** Speculative infrastructure may exist (the unified-tree bet is legitimate), but it must be **quarantined from the default surface**: behind a `cfg(feature = "unstable-…")` flag, off by default. The flag *is* the honesty — it says "this is built ahead of consumers." When a consumer migrates onto it, the flag's exclusions shrink. A speculative surface with *no* flag is dishonest: it claims, by being in the default `pub` API, to be load-bearing. Genuine YAGNI (the `flui-foundation` items — not part of any consolidation intent) is deleted outright.

**Proposed refusal trigger #11:**

> **11. A `pub` module or trait family with zero production (non-test) consumers that is not behind a `cfg(feature = "unstable-*")` gate.**
> **Why:** speculative scaffolding in the default surface mis-signals "load-bearing," inflates the breaking-change radius, and is the dominant cognitive-load tax — a reader cannot find the live 40%. The gate makes the speculation honest.
> **Detection:** `port-check.sh` already has the zero-external-consumer grep methodology the audits used (Appendix A.2 of several). Promote it: a `pub mod` whose every item has zero cross-crate references and which is not `#[cfg(feature = "unstable-...")]`-gated is flagged.
> **Exception, explicit:** crate-internal use counts as a consumer (the cycle-3 nuance). Genuine consolidation infrastructure (`flui-tree` unified surface) satisfies the trigger by being *gated*, not by being deleted.

---

### SP-5 — Lifecycle protocol absent: types that hold resources but have no `Drop` / dispose / dirty-bit

**The pattern.** A type owns a resource — a GPU handle, a registered listener, a tree node that may be reused — but ships with no `Drop` impl, no `dispose()`, no `disposed` flag, no `needs_*` dirty bit. The resource leaks, the dispose-after-use is undetected, or the optimization the type exists for (retained rendering) is structurally impossible.

Flutter's foundation is, to a large extent, *a lifecycle protocol*: `ChangeNotifier.dispose`, `Layer._refCount`/`_unref`/`dispose`, `Layer._needsAddToScene`, `Layer._engineLayer`, `Ticker.dispose`. FLUI ported the *data shapes* and dropped the *protocol*. Ousterhout would call this **temporal decomposition** done wrong — the audit for `flui-layer` names it exactly: "the crate was scaffolded as a *data layer* … without modeling the *resource layer*." The split between "layer as data" and "layer as resource holder" was dropped on the floor.

**Instances:**

| Type | Missing | Consequence |
|---|---|---|
| `flui-layer` `LayerNode` | no `Drop`, no `disposed: AtomicBool`, no `needs_add_to_scene`, no `engine_layer` cache | **No retained rendering.** Every frame re-encodes the entire GPU scene from scratch — `PictureLayer`'s whole reason to exist is void. `set_needs_compositing` exists, zero callers. |
| `flui-scheduler` `Ticker` / `ScheduledTicker` | no `Drop`, no `dispose()`, no `disposed` assert | Awaiters can't detect cancellation; a dropped `ScheduledTicker` with a queued frame callback silently no-ops. The `ChangeNotifier::dispose` template (PR #84) was *not* applied here. |
| `flui-interaction` `GestureBinding.hit_tests` / `pending_moves` / `raw_input` tracking maps | no GC, no last-seen timestamp | `DashMap` entries leak forever if a pointer is dropped without `Up`/`Cancel` (device disconnect, palm rejection). |
| `flui-layer` `LinkRegistry` | `remove_orphaned_followers` exists, **zero callers** | Leader/follower maps grow unbounded across frames when a Scene retains the registry. |
| `flui-engine` `SUPERELLIPSE_CACHE` | (cycle-1 flagged unbounded) | **Closed** (PR #83 — bounded LRU). Listed for completeness of the pattern. |

**Why this is pattern #5.** Four open instances, and the most architecturally consequential of them — `LayerNode` — *defeats the entire point of the layer tree*. The retained-rendering optimization is the reason a layer tree exists (vs. immediate-mode); without `engine_layer` retention, FLUI has paid the cost of a retained layer tree and gets immediate-mode performance. The ecosystem-lessons doc independently flags this: egui/immediate-mode "pay full re-layout every frame" — FLUI is *accidentally* in that bucket for compositing.

**The structural rule.** A type that owns a resource (GPU handle, listener registration, poolable slot) implements the lifecycle protocol: `Drop` (or an explicit `dispose()` + `Drop`-calls-dispose), a `disposed` flag with debug-assert on use-after-dispose, and — for anything in the frame loop — a `needs_*` dirty bit. This is not optional polish; it is the difference between a retained-mode framework and an immediate-mode one wearing a retained-mode tree. FLUI *already has the canonical template* — `ChangeNotifier::dispose` (PR #84), mirrored onto `Ticker` (Cycle 1) and `LayerNode` (Cycle 2 plan). The rule is: apply it *everywhere a resource is owned*, not crate-by-crate as cycles reach them.

*(No new refusal trigger — this is a positive construction rule, not a refusable anti-pattern. It belongs in `docs/PORT.md` as a **construction requirement** under a new "Lifecycle protocol" section: "every resource-owning type implements dispose + disposed-assert + (frame-loop types) a dirty bit.")*

---

### SP-6 — Lock placement and contention: per-field `RwLock`/`Mutex` where an atomic or a snapshot belongs

**The pattern.** State that is read on a hot path, or is a single scalar, sits behind a `RwLock`/`Mutex`. Or: a callback set is cloned-under-lock per notification. Or: an interior-mutability soup (`Arc<RwLock<HashMap<u64, Arc<RwLock<T>>>>>`) is *exposed through a public trait return type*, forcing every consumer to reason about the lock graph.

Flutter is single-threaded; it has *no* locks. FLUI correctly went multi-thread-capable, but the audits show locks applied by reflex rather than by need. The render hot path is "strictly synchronous" per `STRATEGY.md` — refusal triggers 1, 2, 4, 7 already guard the *worst* lock placements. The remaining instances are the ones the triggers don't yet catch.

**Instances:**

| Site | Problem |
|---|---|
| `flui-rendering` `RendererBinding::render_views() -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` | **Triple-lock topology baked into a public trait return type.** Every consumer: `.read()` outer → `.get()` → `.read()` inner. Interior mutability leaked through the abstraction (Ousterhout: information leakage). cycle-4 Wave-2 design reshapes to four typed primitives. Queued. |
| `flui-foundation` `ChangeNotifier::notify_listeners` | clones the entire callback `Vec` under lock per notify — frame-rate allocator pressure for a hot notifier (scroll, animation tick). foundation-tree audit I-4: `SmallVec<[_; 4]>`. |
| `flui-semantics` `SemanticsBinding` | 3–4 separate `RwLock`s; per-event callsites lock 2–3; a 7-bool POD `AccessibilityFeatures` behind a `RwLock` where an `AtomicU8` bitflags fits. |
| `flui-scheduler` `TaskQueue` getters, `Ticker::state()` | lock for trivial scalar reads (`is_empty`, `len`, single-field state). Should be `AtomicU8`/atomic counters. |
| `flui-interaction` `PointerRouter::route` | `2 + N + M` `RwLock::read()` acquisitions per event with linear `Arc::ptr_eq` re-checks — quadratic for >1 handler. Should be single-snapshot dispatch (Flutter's pattern). |
| `flui-engine` `Arc<Mutex<OffscreenRenderer>>`, `Arc<Mutex<TexturePoolInner>>` | single-mutator data behind a lock. Refusal trigger #7 *already* covers this (forward-looking) — known sites file-glob-excluded; the Outstanding refactors land the removal. |

**Why this is pattern #6.** Six sites across five crates. The triggers catch the render-storage hot path; they do not catch *binding-level* lock soup (`render_views`) or *notification-path* allocation (`notify_listeners`) or *event-path* quadratics (`PointerRouter`). And the `render_views` instance is an Ousterhout **information leakage** case study — the lock graph, which should be a hidden implementation detail, is the *public type*.

**The structural rule.** Two sub-rules. (a) A single scalar that is read on a hot path is an atomic, not a lock. (b) A lock is an implementation detail and may not appear in a public type signature — a trait exposes *operations* (`render_view(id) -> Option<Arc<...>>`), never the lock container. Gjengset (*Rust for Rustaceans*, Ch. 3): hide the lock; expose what the lock *does*. The notification-path corollary: snapshot-then-fire with a reusable/`SmallVec` buffer, never clone-the-`Vec`-under-lock.

**Proposed refusal trigger #12:**

> **12. A `RwLock`/`Mutex`/`Arc<RwLock<...>>` appearing in a `pub fn` return type or `pub` field type of a trait/struct (outside the type's own private fields).**
> **Why:** a lock in a public signature leaks the interior-mutability strategy through the abstraction — every consumer must reason about the lock graph, and the strategy can't change without breaking the API.
> **Detection:** `port-check.sh` greps `pub fn` signatures and `pub` trait-method signatures for `RwLock<`/`Mutex<` in return position.
> **Relationship to existing triggers:** 1/2/4/7 catch lock *placement on the render hot path*; #12 catches lock *exposure through any public API*. Complementary, non-overlapping.

---

### SP-7 — Pass-through and parallel-hierarchy indirection: shallow modules that only forward

**The pattern.** A trait, a method, or a whole base-class chain exists that adds no behavior — it only forwards to something else, or it mirrors Flutter's inheritance hierarchy as Rust traits that nothing implements. Ousterhout names this precisely: **pass-through methods** ("a method that does nothing except pass its arguments to another method with a similar signature") and **shallow modules** (interface as complex as the implementation). His remedy: a shallow module is worse than no module — it adds an interface to learn for zero abstraction gained.

**Instances:**

| Site | Why it's pass-through / shallow |
|---|---|
| `flui-interaction` `OneSequenceGestureRecognizer` + `PrimaryPointerGestureRecognizer` traits (823 LOC) | Direct transliteration of Flutter's abstract-class chain. **Zero `impl` blocks** — all 7 concrete recognizers roll their own state. The traits are a Flutter hierarchy ported as Rust traits that mirror the hierarchy and nothing else. (interaction audit: either *migrate the 7 recognizers onto them* — no-quick-wins — or delete.) |
| `flui-rendering` `RenderObject::insert_into_pipeline` (R-21) | trait method that requires `Self: Sized` + a `From` bound on a *different* type, wrapping a one-line `owner.insert(box self)`. Pollutes the trait for a convenience a free function would carry. |
| `flui-painting` two `ClipContext` traits (pre-PR#82) | parallel traits, both zero production impls. **Closed** (PR #82) — listed as the pattern's exemplar of resolution. |
| `flui-painting` `canvas/sugar/` (720 LOC, 30+ methods) | every method forwards to a primary `draw_*`. Pure pass-through ergonomics, zero callers. |
| `flui-scheduler` `PriorityExt`/`FrameBudgetExt`/`FrameTimingExt` (150 LOC) | extension traits whose methods duplicate inherent methods on the same types. |
| `flui-rendering` re-export-only `mod.rs` files | the audit's own verdict: *leave* — these are facade modules that genuinely group several types. **Listed to mark the boundary**: a re-export module that aggregates is fine; a *trait* that only forwards is not. |

**Why this is pattern #7.** It is the most *Ousterhout-canonical* of the eight — pass-through methods and shallow modules are his named examples. The Flutter-port context makes FLUI especially prone: porting Flutter "behavior loyal, structure Rust-native" (STRATEGY) means the *structure* should diverge, but a literal transliteration of Flutter's class hierarchy into trait hierarchy produces exactly this — `OneSequenceGestureRecognizer` is Flutter's `OneSequenceGestureRecognizer` abstract class as a Rust trait, and Flutter's class earns its place via inheritance that Rust does not have.

**The structural rule.** A trait must add behavior or a contract that callers depend on polymorphically. A trait with zero `impl`s, or a method that only forwards, is deleted — the behavior moves to a free function or an inherent method. Flutter's abstract-class chains are *not* ported as trait chains by default; they are ported as whatever Rust shape carries the *behavior* (often: one trait + concrete types, or composition, per `STRATEGY.md`). This is the architectural-contracts doc's Contract 2/6 territory.

*(No new refusal trigger — refusal trigger #11 already catches the zero-consumer case, which covers the worst instances. The pass-through-method case is a code-review judgment, not mechanizable. It belongs in `docs/PORT.md` as a mapping rule: "Flutter abstract-class chains port to behavior-carrying Rust shapes, not to mirror trait hierarchies.")*

---

### SP-8 — Constructor-time panics and `from_u8` panics on the public surface

**The pattern.** A `pub fn` — often a constructor or an enum-from-primitive — calls `.expect()`/`panic!`/`unimplemented!()` on input that can come from user code or from a round-tripped atomic load. The Constitution (Principle 6) forbids `unwrap()`/`panic!` in library code; these are the same thing wearing a constructor.

This is a narrower, more local pattern than SP-1, and it overlaps it — but it is worth separating because the *fix* is different. SP-1 is "a phase does nothing"; SP-8 is "a function aborts the process on bad input." Ousterhout's **"define errors out of existence"** is the lens: many of these panics can be designed away (take a `NonZeroU64` instead of a `u64`, so zero is unrepresentable) rather than reported.

**Instances:**

| Site | Panic |
|---|---|
| `flui-rendering` `SemanticsBuilder::new` | `unimplemented!()` on construction; `Default` impl makes it reachable via `derive(Default)`. (cycle-4 R-3; *partially* — PR#117 era addressed the owner/binding `unimplemented!()`s; verify this one.) |
| `flui-interaction` `FocusNodeId::new(0)`, `HandlerId::new(0)` | `.expect("cannot be 0")` — `HandlerId` has no `try_new` escape hatch at all. |
| `flui-scheduler` `SchedulerPhase::from_u8`, `FrameSkipPolicy::from_u8`, `AppLifecycleState::from_u8` | `panic!` on invalid value — called from raw atomic loads in production paths. |
| `flui-scheduler` `VsyncScheduler::new(0)`, `FrameDuration::from_fps(0)`, `set_time_dilation(<=0)` | `assert!` panics. |
| `flui-foundation` `Key::from_str` | `const fn` returns `Key(1)` silently on a zero-hash — silent collision, not a panic, but the same "bad input handled wrong" class. |
| `flui-view` `attach_root_widget` | `assert!` panic on double-attach (a `#[should_panic]` test pins it as "documented"). |

**Why this is pattern #8.** Roughly a dozen sites across four crates, all on the *public* surface. The widget catalog will call constructors constantly; a `pub fn new` that aborts the process on a bad argument is a foot-gun handed to every widget author. The cycle audits flag each individually; as a *class* it wants one rule.

**The structural rule.** Two-part. (a) Prefer **defining the error out of existence**: a constructor that cannot accept zero takes `NonZeroU64`, not `u64` — the bad input is unrepresentable, no panic needed. (b) Where the input genuinely can be bad (a `from_u8` over a wire value, a round-tripped atomic), return `Result`/`Option` and let the caller handle it — never `panic!`. A `pub fn` in a library crate may not abort the process on its argument.

**Proposed refusal trigger #13:**

> **13. `unwrap`/`expect`/`panic!`/`unimplemented!`/`assert!` reachable from a `pub fn` on its arguments, in a library crate.**
> **Why:** a public constructor or conversion that aborts on bad input is a process-abort foot-gun in every consumer; the widget catalog will call these constantly. Per Constitution Principle 6 — this trigger *operationalizes* Principle 6 at the `pub fn` boundary.
> **Detection:** `port-check.sh` greps `pub fn` bodies (transitively, shallow) for the five forms, excluding `#[test]` and `#[cfg(test)]`. A `debug_assert!` is permitted (release-elided invariant check); a plain `assert!` on an argument is not.
> **Relationship:** this is the enforcement arm of Constitution Principle 6, which today is a prose principle with no `port-check.sh` gate.

---

### Systemic-pattern summary

| # | Pattern | Instances | New trigger | Severity to the widget catalog |
|---|---|---|---|---|
| SP-1 | Stubbed-but-called (no-op/panic on hot path) | 11 | **#8** | **Fatal** — layout + composite phases are stubbed |
| SP-2 | Written-but-uncalled (wrong impl is live) | 5 | **#9** | **Fatal** — keyed reconciler is the uncalled one |
| SP-3 | Parallel cross-crate types | 11 pairs (~4 closed) | **#10** | High — change amplification across the catalog |
| SP-4 | Speculative scaffolding (0-consumer surfaces) | 7 crates, >20k LOC | **#11** | High — cognitive load; mis-signals "done" |
| SP-5 | Lifecycle protocol absent | 4 open | (construction rule) | High — defeats retained rendering |
| SP-6 | Lock placement / public-API lock exposure | 6 | **#12** | Medium — contention + information leakage |
| SP-7 | Pass-through / parallel-hierarchy indirection | ~5 | (covered by #11 + mapping rule) | Medium — shallow modules |
| SP-8 | Constructor / `from_u8` panics on `pub` surface | ~12 | **#13** | Medium — foot-gun per widget author |

Six proposed refusal triggers (#8–#13). The systemic finding: **of the eight patterns, six are mechanically detectable and become standing lints; two (SP-5, SP-7) are positive construction rules / mapping rules.** Installing the six triggers is the single highest-leverage architectural action in this plan — it converts "fix the defects" into "the defects cannot recur," which is the only durable answer to a tactical tornado.

---

## 2. How the tiers are defined

- **P0 — fix before *any* user-facing construction begins.** A defect is P0 if it makes the foundation *fragile under load* in a way the widget-catalog author *cannot see*. These are the unknown-unknowns that compound across ~80 widgets / ~480k LOC. The test: "would a widget built on this look correct in a demo and be silently broken in production?" If yes → P0.
- **P1 — fix before the foundation is declared stable; may overlap early catalog work.** Real defects, but either visible (loud failure, the author notices) or not on the universal path (semantics, a single widget family). Building a *little* catalog on a P1-defective foundation is survivable; building *all* of it is not.
- **P2 — hygiene; fix opportunistically or as the owning crate is next touched.** Cognitive-load and dead-weight defects. They do not break anything; they make the workspace harder to reason about and slower to compile. Real, but not gating.

The asymmetry the architectural-contracts doc names applies: a defect's tier is driven by *blast radius and visibility*, not by fix cost. A cheap-to-fix silent defect on the universal path (the reconciler) is P0; an expensive-to-fix loud defect off the path is P1.

---

## 3. P0 — fix before any widget-catalog construction

Every P0 defect is a member of SP-1 or SP-2 — the two unknown-unknown patterns — *or* it is the contract-level decision that the architectural-contracts doc independently ranked "MUST LOCK before construction." Per-defect: site, what's wrong, complexity symptom, root cause, fix, blast radius, what it blocks.

### D-1 — The layout phase runs nothing (`layout_node_with_children` never calls `RenderEntry::layout`)

- **Site:** `crates/flui-rendering/src/pipeline/owner.rs` `layout_node_with_children` (~line 855); confirmed against HEAD source.
- **What's wrong:** the function walks the tree depth-first checking `needs_layout()` and recursing, but **never invokes `RenderEntry::layout(constraints)`** — the only path that runs `RenderObject::perform_layout_raw`. The audit's R-13 flagged empty-body `propagate_constraints_to_child`/`sync_child_size_to_parent`; cycle-4 Wave-5 *deleted those stubs subtractively*, and the in-code comment now states plainly: per-node layout "happens nowhere in production today; the only `entry.layout()` callsite is inside a `#[test]`."
- **Complexity symptom:** **SP-1 / unknown-unknowns.** The build phase produces a correct render tree; then nothing lays it out; `run_paint` paints zero-sized nodes. A demo with one hard-coded-size widget might appear to work; nothing real does.
- **Root cause:** the layout walk was built before `RenderEntry::layout` was the settled per-node entry point, and the wiring of walk→entry was never completed — a tactical tornado: the recursion shape was kept "so when the per-node call lands the traversal is in place," but the call never landed.
- **Fix:** in `layout_node_with_children`, after the depth-first child recursion (post-order, so child sizes are available — the shape is already correct), call `entry.layout(constraints)` for each node, with constraints propagated parent→child per Flutter's `performLayout`. The constraint-propagation logic that R-13's deleted stubs were *supposed* to carry must be written, not re-deleted.
- **Blast radius:** `owner.rs` only (~40–60 LOC). Self-contained.
- **Blocks:** **everything.** No widget lays out without this. It is the first true bottleneck of the entire port phasing.

### D-2 — Index-not-key reconciliation: `VariableChildStorage` uses the positional loop; the keyed reconciler is dead

- **Site:** `crates/flui-view/src/element/child_storage.rs` `VariableChildStorage::update_with_views` (lines ~494–515, verified) — 21-LOC positional `for (i, view) in views.iter().enumerate()`. The correct algorithm: `crates/flui-view/src/tree/reconciliation.rs::reconcile_children` (325 LOC, full keyed O(N) linear) — verified production callers = **0**.
- **What's wrong:** child elements are matched to new views *by index*. On any list reorder, every element rebuilds from scratch — element identity (and therefore `State`, scroll offset, focus, animation) is lost. `GlobalKey` reparenting, `Hero`, `Reorderable`, `ListView` state preservation all break. The architectural-contracts doc ranks this Contract 5, "MUST LOCK," and names it a "silent-correctness trap … the most dangerous kind."
- **Complexity symptom:** **SP-2 / unknown-unknowns.** The correct reconciler is *written and tested* and *sitting unused*. A static list reconciles fine positionally — demos pass. The defect surfaces only on reorder, in user apps, not in FLUI's tests.
- **Root cause:** tactical tornado — `reconcile_children` was written; wiring it required adding a `key` field to `ElementNode` (the audit's prerequisite); the 21-line positional loop was put in to make `Variable`-arity elements work *now*; nobody returned.
- **Fix:** (1) add `key: Option<flui_foundation::Key>` to `ElementNode`, set at insert from `View::key()` (the `registered_global_key_hash` side-channel is half of this already); (2) `update_with_views` delegates to `reconcile_children`; (3) fix `reconciliation.rs:91-98`'s "we don't have access to the original View's key" gap now that the key is stored; (4) unify slot handling on `IndexedSlot`. The architectural-contracts doc notes this should be co-designed with Contract 3 (heterogeneous children) — a tuple-based `ViewSeq` spine makes the contiguous fast-path monomorphic.
- **Blast radius:** `flui-view` — `ElementNode`, `child_storage.rs`, `reconciliation.rs`; ~bounded, the keyed algorithm already exists. Far cheaper now (0 list widgets) than after the catalog.
- **Blocks:** every list/grid/table widget, `Hero`, `Reorderable`, `GlobalKey` reparenting, animated-list state.

### D-3 — `run_compositing` is a no-op

- **Site:** `crates/flui-rendering/src/pipeline/owner.rs` `run_compositing` (~line 922, verified) — sorts the dirty list, logs `"compositing-bits update is a no-op until..."`, clears the list, returns `Ok(())`.
- **What's wrong:** Flutter's `flushCompositingBits` walks dirty nodes and sets each layer's compositing-needs flag (via `_updateSubtreeCompositingBits`). FLUI does none of it. Callers get `Ok(())`.
- **Complexity symptom:** **SP-1 / unknown-unknowns** — the cycle-4 audit's own words: "Worse than R-1 [the panic] because … this is silent."
- **Root cause:** the comment claimed the blocker was "PipelineOwner needs a reference to RenderTree" — *false*, `PipelineOwner` holds `render_tree` inline. The stub outlived its own stated rationale.
- **Fix:** implement the subtree compositing-bits walk per Flutter `object.dart::flushCompositingBits`. The cycle-4 audit (R-4) gives a structurally-correct skeleton.
- **Blast radius:** `owner.rs` only (~30 LOC).
- **Blocks:** correct compositing — without it, layers that *need* a compositing layer (opacity, clip, filter, shader-mask under a `RepaintBoundary`) may composite wrong. Paint "works for the common case" today; the catalog's effect widgets do not stay in the common case.

### D-4 — `run_paint` clears `needs_paint` for nodes it never paints

- **Site:** `crates/flui-rendering/src/pipeline/owner.rs` `run_paint` (~line 983, verified).
- **What's wrong:** the flag-clear loop iterates the dirty list clearing `needs_paint`; the paint *walk* descends only from `root_id`. A node-needing-paint not reachable from `root_id` in that descent gets its flag **cleared without being painted** — it stays stale until something else dirties it.
- **Complexity symptom:** **SP-1.** The clear-pass and the paint-pass disagree on which nodes are painted. Single-root keeps it hidden; a detached subtree or `RepaintBoundary`-isolated repaint exposes it.
- **Root cause:** the audit's R-15 — a `// Note: We don't need to sort for now since we paint from root` comment that quietly commits to a single-root invariant the architecture never promised.
- **Fix:** fold the flag-clear into `paint_node_recursive` (clear when painted, like Flutter's `flushPaint`); sort the dirty list deep-first; `tracing::warn!` any node-needing-paint not reached. R-15 gives the shape.
- **Blast radius:** `owner.rs` only (~20 LOC).
- **Blocks:** correct incremental repaint — every `RepaintBoundary`-based optimization the catalog will lean on for scroll/animation performance.

### D-5 — Lock the three "MUST LOCK" contracts that the widget `impl` surface commits to at widget #1

This is not one code site — it is the architectural-contracts doc's top-ranked finding, folded in because it is genuinely P0: these are decisions the *first widget written* bakes in, and changing them after the catalog exists is a catalog-wide rewrite. The contracts doc is the authority; this plan ratifies its ranking and ties it to the systemic patterns.

- **D-5a — Heterogeneous children ergonomics (Contract 3).** `children` is the field of `Column`/`Row`/`Stack`/`Wrap`/`Flex`/`ListView`/`Table` — the spine of every real UI. Current `Children`(`Vec<BoxedView>`) is builder-only, no `[...]` literal, `dyn_clone` per frame. **Decision to lock:** a `ViewSeq` tuple trait + `column!`/`row!` macros, `Vec<BoxedView>` retained as the dynamic fallback. *Symptom:* this is the one place FLUI must *not* port Flutter's structure (`List<Widget>` cannot be a Rust `Vec`). Needs its own `/speckit.plan`.
- **D-5b — Widget-authoring API (Contract 6).** `build()` must return `impl IntoView`, not `Box<dyn View>`; a `#[derive(StatelessView)]` (or coherent blanket impl) must remove the hand-written `impl View` block; `bon` for many-field constructors. *Symptom:* SP-7-adjacent — the current 3-step ritual is shallow ceremony. The most-touched public surface in the framework; the adoption-metric driver. Needs its own design doc.
- **D-5c — `View` trait surface + element storage (Contract 2/5).** Lock the object-safe `View` trait signature; reshape element storage from `Box<dyn ElementBase>` toward an `enum ElementNode` (closed set of behaviors) so the failing runtime `downcast_ref::<V>()` in the update path (`generic.rs:271`) becomes a typed match-arm. *Symptom:* SP-1-adjacent — a `downcast` that "should be impossible" is a `tracing::warn!` instead of a compile error. Co-design with D-2 (the keyed reconciler dispatches on this storage).

**Why D-5 is P0:** the contracts doc's argument is decisive — "the catalog commits to one branch at widget #1." A `ViewSeq` retrofit after `flui-material` exists re-types the `children` field of every multi-child widget and rewrites every example. The *decision* is cheap now; the *rewrite* is catalog-wide later. **Blast radius if deferred:** the entire catalog. **Blocks:** the catalog cannot start without these locked — not because code is missing but because the wrong lock poisons every widget built before the correction.

### D-6 — Verify the three `unimplemented!()` closures are fully closed; install trigger #8

- **Site:** `flui-rendering` `run_semantics`, `RendererBinding::perform_semantics_action`, `SemanticsBuilder::new` (cycle-4 R-1/R-2/R-3).
- **State at HEAD:** PR #117 (cycle-4 Wave-5) closed the loud macros — verified: only doc-comment *mentions* of `unimplemented!()` remain in `owner.rs`/`binding/mod.rs`. `SemanticsBuilder::new` in `delegates/custom_painter.rs` — the doc-comment still describes it as a placeholder; **needs a direct verify** that the body no longer panics.
- **Why still P0:** not the residual code — the *rule*. Refusal trigger #8 (SP-1) must be *installed and made a `port-check.sh` gate* before catalog construction, so the eleven SP-1 instances cannot regrow and so D-1/D-3/D-4 are *caught* if a future change re-stubs them. Installing the trigger is the P0 deliverable; closing `SemanticsBuilder::new` is a one-line check folded in.
- **Blast radius:** `port-check.sh` + a `// STUB-OK` allowlist; `delegates/custom_painter.rs` if the builder still panics.
- **Blocks:** nothing functionally — but it is the *gate* that keeps P0 closed. Without it, P0 is a snapshot, not a guarantee.

### P0 — summary table

| ID | Defect | Crate | Symptom | Blast radius | Blocks |
|---|---|---|---|---|---|
| D-1 | Layout phase invokes no per-node `layout()` | flui-rendering | SP-1 | owner.rs (~50 LOC) | **all layout** |
| D-2 | Index-not-key reconciliation; keyed reconciler dead | flui-view | SP-2 | flui-view (ElementNode + 2 files) | all list/grid/Hero/keyed |
| D-3 | `run_compositing` no-op | flui-rendering | SP-1 | owner.rs (~30 LOC) | correct compositing / effect widgets |
| D-4 | `run_paint` clears unpainted nodes' flags | flui-rendering | SP-1 | owner.rs (~20 LOC) | incremental repaint / RepaintBoundary |
| D-5a | Heterogeneous-children contract unlocked | flui-view | SP-7-adj | the catalog | all multi-child widgets |
| D-5b | Widget-authoring contract unlocked | flui-view | SP-7-adj | the catalog | every widget + adoption |
| D-5c | `View` trait / element storage contract unlocked | flui-view | SP-1-adj | the catalog | every widget; co-design w/ D-2 |
| D-6 | Install trigger #8; verify `SemanticsBuilder::new` | port-check.sh | SP-1 | tooling | keeps P0 closed |

**P0 count: 8 defects** (D-5 counted as three contract locks, since each is an independent commitment). Six are concrete code/contract fixes; D-6 is the gate that makes the tier durable.

---

## 4. P1 — fix before the foundation is declared stable

Real defects; either loud (the author notices) or off the universal path. Condensed — site, wrong, symptom, fix, blocks.

### D-7 — Layer lifecycle protocol absent → no retained rendering
- **Site:** `flui-layer` `LayerNode` — no `Drop`/`disposed`/`needs_add_to_scene`/`engine_layer`. **Symptom:** SP-5. **Wrong:** every frame re-encodes the whole GPU scene; the layer tree's reason to exist is void. **Fix:** the layer-semantics repair plan's phased introduction (`disposed`+`Drop` → `needs_add_to_scene` propagation → `engine_layer` cache). **Why P1 not P0:** widgets are *correct* without it, just slow — the catalog can be *built and tested headless* on an un-retained compositor; the perf cliff bites at real-app time. **Blocks:** 60fps under real load.

### D-8 — Parallel cross-crate types: `RenderError`, hit-test trio, painting `PointerEvent`, `Color`
- **Site:** SP-3 instances not yet landed. **Symptom:** SP-3 / change amplification. **Fix:** cycle-4 Wave-2 design (hit-test trio — queued); rename `flui-engine::RenderError → EngineError`; delete `flui-painting::display_list::hit_region`; delete `flui-app::theme::colors`. **Why P1:** loud (name collisions are compile errors the moment both are imported) — visible, not silent. **Blocks:** clean catalog imports; a Material widget importing both `flui-rendering` and `flui-engine` preludes hits the `RenderError` ambiguity.

### D-9 — `BuildContext` `new_minimal` correctness hole (Contract 4)
- **Site:** `flui-view` `StatelessBehavior::perform_build` builds a *minimal* `ElementBuildContext` not wired to the tree — so `ctx.depend_on::<Theme>()` inside a real `build()` cannot reach the `inherited_elements` registry. **Symptom:** SP-1-adjacent — the context *looks* functional. **Note:** the registry itself (cycle-5 V-3) **is now wired** — verified `register_inherited` is called at `build_owner.rs:651`. The remaining hole is the *build-time context*. **Fix:** delete the `new_minimal` build path; the wired context reaches `build()`. **Why P1:** theming is needed by ≈ Material widget #1 — but `flui-widgets` layout/scroll/input widgets can be built and tested *before* the first themed widget. Close before `flui-material`. **Blocks:** every `InheritedWidget`-consuming widget; `Theme.of`.

### D-10 — Tab navigation is a stub; two parallel `FocusManager`s
- **Site:** `flui-interaction` `FocusManager::focus_next` (`tracing::warn!`) vs the working unused `FocusScopeNode::focus_next_in_scope`. **Symptom:** SP-1 + SP-2. **Fix:** the interaction-scheduler audit's consolidation — `FocusManager` delegates to the tree-based `FocusManagerInner`; implement `focus_next` over the scope hierarchy; delete the flat state. **Why P1:** loud-ish (the warn fires) and off the universal layout path. **Blocks:** keyboard-navigable Material/Cupertino.

### D-11 — `TreeWrite::remove` codifies the non-cascade footgun
- **Site:** `flui-tree` `traits/write.rs` — `remove` doc says children "may be orphaned"; `RenderTree::remove` orphans descendants in the slab. **Symptom:** SP-5-adjacent (lifecycle) — a removed subtree leaks; slab-index reuse → silent tree corruption. **Fix:** the foundation-tree audit's T-3 — hoist cycle-2's `remove` (cascade default) + `remove_shallow` (opt-out) into the `TreeWrite` trait; `RenderTree` inherits the fix. **Why P1:** untested today (the render tree rarely mutates subtrees yet) — but the catalog *will* mutate subtrees constantly. Fix before catalog mutation load. **Blocks:** safe subtree removal under reconciliation.

### D-12 — Ticker lifecycle leak; `from_u8` panics; constructor panics (SP-5 + SP-8)
- **Site:** `flui-scheduler` `Ticker`/`ScheduledTicker` (no `Drop`/`dispose`), `*::from_u8` panics, `VsyncScheduler::new(0)` etc.; `flui-interaction` `FocusNodeId::new(0)`/`HandlerId::new(0)`. **Symptom:** SP-5 + SP-8. **Fix:** apply the `ChangeNotifier::dispose` template to `Ticker`; `from_u8 → Option`; constructors take `NonZero*` or return `Result`. **Why P1:** `flui-animation` (the Ticker consumer) is disabled — the leak is latent until animation re-enables (port-phasing Phase 2). Fix as part of the animation re-entry. **Blocks:** correct `AnimationController` lifecycle.

### D-13 — `BuildContext` callback surface is right — ratify and lock it
- **Site:** `flui-view` `BuildContext` — the callback-form `depend_on_inherited`/`find_ancestor_*` (`build_context.rs`). **Not a defect** — the architectural-contracts doc (Contract 4) finds it *correct*. Listed in P1 because it must be *explicitly declared stable* before the catalog so no widget reaches for a different shape, and `Send + Sync` should be dropped from `BuildContext` (build is single-threaded — a free bound relaxation). **Fix:** documentation + the `Send+Sync` removal. **Blocks:** nothing; prevents future churn.

### P1 — summary

| ID | Defect | Crate | Symptom | Why P1 (not P0) |
|---|---|---|---|---|
| D-7 | Layer lifecycle absent → no retained render | flui-layer | SP-5 | correct-but-slow; testable headless |
| D-8 | Parallel `RenderError`/hit-test/`PointerEvent`/`Color` | rendering/engine/painting/app | SP-3 | loud (compile-error collisions), visible |
| D-9 | `BuildContext` `new_minimal` hole | flui-view | SP-1-adj | needed by Material #1, not widget #1 |
| D-10 | Tab nav stub; two `FocusManager`s | flui-interaction | SP-1+SP-2 | off universal path |
| D-11 | `TreeWrite::remove` non-cascade footgun | flui-tree | SP-5-adj | untested today; bites under catalog load |
| D-12 | Ticker leak + `from_u8`/ctor panics | flui-scheduler/interaction | SP-5+SP-8 | consumer (`flui-animation`) disabled |
| D-13 | Ratify+lock `BuildContext` surface | flui-view | — | not a defect; lock to prevent churn |

**P1 count: 7** (D-13 is a ratification, not a fix — 6 actual defects).

---

## 5. P2 — hygiene; fix opportunistically

Dead-weight and cognitive-load defects. None gate construction. Grouped by systemic pattern; the per-crate audits carry the line-level inventory.

- **P2-A — Speculative scaffolding (SP-4), >20k LOC.** Feature-gate or delete per the audit verdicts: `flui-tree` ~10,600 LOC behind `unstable-tree` (migration gap, *gate not delete* per memory); `flui-view` ~1,965 LOC behind `unstable-views`; `flui-painting` delete `tessellation`+`hit_region`, gate `canvas-sugar`+`text-painter`; `flui-rendering` gate `experimental-delegates`+`experimental-scroll`; `flui-scheduler` delete `typestate.rs`+`Handle<M>`+ZST-priorities+`prelude_advanced`+`VsyncDrivenScheduler`+`arc_instance`; `flui-interaction` delete `typestate.rs`, *migrate-or-delete* the recognizer base traits, gate `testing/`; `flui-foundation` delete `ObserverList`+`FoundationError`+`FluiError`+`MergedListenable`+`HashedObserverList` (genuine YAGNI — drop the `dashmap` dep). **Installing trigger #11 is the durable fix** — it forces the gate-or-delete decision and prevents regrowth.
- **P2-B — Lock placement not on the render hot path (SP-6).** `ChangeNotifier::notify_listeners` → `SmallVec` snapshot; `SemanticsBinding` 4 `RwLock`s → `AtomicU8` bitflags + clone-and-release; `flui-scheduler` scalar getters → atomics; `PointerRouter::route` → single-snapshot dispatch. Trigger #12 covers the public-API-exposure subset.
- **P2-C — Constructor / scalar-read panics (SP-8) not on the universal path.** `Key::from_str` zero-hash collision; `attach_root_widget` `assert!` → `Result`. Trigger #13 enforcement.
- **P2-D — Allocation hot paths (flui-painting alloc audit).** `Paint::clone` per `draw_*` (interning — blocked on `Paint: Hash+Eq`); `Path::clone` per `draw_path` (Cow — blocked on `flui-types` change); per-`DrawCommand` 64-byte `Matrix4` baking (flat-bytecode — high blast radius). All have *named external blockers* — legitimately deferred per the alloc audit; revisit when the blockers clear. **Not** quick-wins-deferral; genuine dependency blockers.
- **P2-E — Depth-constant fragmentation (SP-adjacent).** `flui-tree` has 4+ independent "tree depth" constants (`MAX_TREE_DEPTH=256`, `TreeNav::MAX_DEPTH=32`, `TreeVisitor::MAX_STACK_DEPTH=64`, `STACK_SIZE=48`, plus inline SmallVec sizes). Unify behind one `MAX_TREE_DEPTH` cap + one `DEFAULT_INLINE_DEPTH` sizing hint. Cognitive load; a deeper-than-32 tree needs N independent fixes today.
- **P2-F — `flui-types` compile-time tax.** 36k LOC, 3× Flutter's whole `foundation`, dependency of everything — every edit triggers a near-workspace rebuild. The port-phasing doc (R7) recommends a post-Phase-3 split along Flutter's seams (geometry / painting-values / styling). **Velocity, not correctness** — defer, do not block. *(This overlaps the sibling crate-decomposition analysis — flagged for that doc.)*
- **P2-G — Engine dead code.** ~2,800 LOC verified-dead in `flui-engine` (`PipelineManager`/`PipelineHandle` zombies, `pipeline.rs` vs `pipelines.rs` dual namespace, forward-looking `effects`/`instancing` helpers). Delete per cycle-4 E-findings.
- **P2-H — Doc drift.** `CLAUDE.md` "Current Development Focus" lists crates as disabled that are active and vice versa (the gap-matrix and several audits note this). Cosmetic; fix on next `CLAUDE.md` touch.

---

## 6. Mythos-coverage delta — covered vs new

The Mythos cycle remediates crate-*pairs*. This ledger states, per defect, whether an in-flight/queued Mythos plan already covers it, or whether it is **NEW** and needs its own plan.

### Covered by landed Mythos work (verified against HEAD)
- The three `unimplemented!()` macros (R-1/R-2/R-3) — **closed**, PR #117 (cycle-4 Wave-5). D-6 residual = verify `SemanticsBuilder::new` + install the trigger.
- `ParentData` parallel type (R-11) — **closed**, PR #84.
- `ViewKey` parallel trait — **closed**, PR #84.
- `ClipContext` parallel traits — **closed**, PR #82.
- `SUPERELLIPSE_CACHE` unbounded — **closed**, PR #83.
- `BuildOwner::inherited_elements` registry wiring (cycle-5 V-3) — **closed**: `register_inherited` is called at `build_owner.rs:651`. (D-9's *residual* — the `new_minimal` build-time context — is NOT closed.)
- Empty-body `propagate_constraints_to_child`/`sync_child_size_to_parent` (R-13) — **deleted**, cycle-4 Wave-5 — but *subtractively*: D-1 (layout calls nothing) is the hole left behind. **Coverage is partial and misleading** — the stubs are gone, the layout phase still does nothing.

### Covered by queued/in-flight Mythos plans (designed, not landed)
- D-8 hit-test trio (`HitTestResult`/`MouseTrackerAnnotation`/`MouseTracker`) + `RendererBinding` lock topology — **covered** by `docs/research/2026-05-22-cycle4-wave2-design.md` (cycle-4 Wave-2). Backdrop-filter command path (E-2) also there.
- D-7 layer lifecycle + enum boxing + `SemanticsService::send_event` — **covered** by the layer-semantics repair plan (`2026-05-22-004`, cycle-2 follow-up).
- The cycle-5 painting×view zombie feature-gating (P2-A subset for those two crates) — **covered** by the cycle-5 plan (audited, plan pending per memory `[[flui-cycle5-painting-view-inflight]]`).

### NEW — needs its own plan (not in any Mythos cycle)
- **D-1 — layout phase wiring.** Cycle-4 audited the *stubs* (R-13) and deleted them; **no plan covers writing the actual per-node layout call.** This is the single most important NEW item. The widget catalog cannot start without it. NEW PLAN REQUIRED.
- **D-2 — keyed reconciliation wiring.** Cycle-5 audited it (V-4) — but the cycle-5 *plan* status is "audited, plan pending"; and V-4 as scoped is "hoist `reconcile_children`," which understates the `ElementNode` key-field prerequisite. Needs explicit plan coverage, co-designed with D-5a/D-5c. NEW PLAN REQUIRED (or expanded cycle-5 scope).
- **D-3 — `run_compositing` implementation.** Cycle-4 R-4 gave a skeleton; **no Wave landed it** (Wave-5 closed the audit's other findings). NEW.
- **D-4 — `run_paint` dirty-list fix.** Cycle-4 R-15; **not landed.** NEW.
- **D-5a/b/c — the three contract locks.** The architectural-contracts doc explicitly says these need *standalone `/speckit.plan`s* — they are roadmap-foundations work, **not Mythos** (Mythos hardens existing crates; these design the *new* widget surface). NEW — three design docs.
- **The six refusal triggers (#8–#13).** Mythos *uses* the triggers; it does not *author* them. Installing #8–#13 in `docs/PORT.md` + `port-check.sh` is a NEW deliverable — and the highest-leverage one, because it converts every systemic pattern from "fix" to "cannot recur."
- **D-11 — `TreeWrite::remove` cascade contract.** The foundation-tree audit (cycle-3, T-3) flagged it; **cycle-3 PR #103 hoisted cascade to `TreeWrite::remove` for the trait** — *verify*: the cycle-4 audit says "RenderTree adopts it cleanly … already inherited." If verified, D-11 is **covered**; if the trait-doc still says "may be orphaned," NEW. (Treated as P1 pending that one verification.)
- **D-12 — Ticker lifecycle + scheduler panics.** The scheduler audit is a *draft* (`2026-05-21-flui-scheduler-audit-draft.md`) — no cycle has executed it. NEW (or: promote the draft to a cycle).

### Delta summary
- **Covered (landed):** 6 defect-classes + R-13 *partially* (misleadingly).
- **Covered (queued):** 3 (hit-test trio, layer lifecycle, cycle-5 zombies).
- **NEW — needs a plan:** **D-1, D-3, D-4** (rendering phase implementations — no Wave covers them), **D-2** (needs expanded scope), **D-5a/b/c** (three contract design docs), **the six refusal triggers**, **D-12** (scheduler draft un-executed). D-11 pending one verification.

The headline of the delta: **Mythos has been auditing and closing *hygiene and parallel-type* defects effectively, but the three stubbed render *phases* (D-1, D-3, D-4) — the SP-1 instances that block all of layout and composite — are NOT on any Mythos Wave's landing schedule.** Cycle-4 *audited* them (R-4, R-13, R-15) and either deleted the stub subtractively or left a skeleton. **The most dangerous defects in the workspace are audited but unowned.** That is the single most important thing this delta surfaces for the FOUNDATIONS doc.

---

## 7. What must be true before widget-catalog construction starts — the gate

A binary checklist. Every item must be *true* (not "planned") before the first `flui-widgets` widget is written. Derived from the P0 tier and the systemic patterns.

1. **Layout runs.** `layout_node_with_children` invokes `RenderEntry::layout` per node with constraints propagated parent→child. A `Padding → Center → ColoredBox` integration test proves a 3-level tree lays out with correct constraints and sizes. *(D-1)*
2. **Reconciliation is keyed.** `ElementNode` carries a `key`; `VariableChildStorage::update_with_views` delegates to `reconcile_children`; the positional loop is deleted. A test proves `[A(key=1), B(key=2)]` reordered to `[B, A]` preserves element identity (no remount). *(D-2)*
3. **Compositing runs.** `run_compositing` performs the subtree compositing-bits walk; it is not a no-op. *(D-3)*
4. **Paint and dirty-clear agree.** `run_paint` clears `needs_paint` only for nodes it paints; deep-first sorted; unreached dirty nodes are logged. *(D-4)*
5. **The three "MUST LOCK" contracts are locked, with design docs.** Heterogeneous-children (`ViewSeq` + `column!`), widget-authoring (`build() -> impl IntoView` + `#[derive]` + `bon`), and `View`-trait/element-storage each have an approved `/speckit.plan`. *(D-5a/b/c)*
6. **Zero stubbed-but-called methods on the render path.** Refusal trigger #8 is installed in `docs/PORT.md` and is a green `port-check.sh` gate; the `// STUB-OK` allowlist is empty or every entry has a tracking issue. `SemanticsBuilder::new` does not panic. *(D-6, SP-1)*
7. **No silent parallel implementations of a load-bearing algorithm.** Refusal trigger #9 installed; no responsibility has a wrong-impl-live / correct-impl-dead pair on a production path. *(SP-2)*
8. **The six refusal triggers (#8–#13) are written into `docs/PORT.md`** and the mechanically-detectable ones (#8, #10, #12, #13) are `port-check.sh` gates. This is what makes the gate *durable* rather than a snapshot — without it, the patterns regrow as the catalog is written.
9. **The render machine's `Scene`/`DrawCommand` contract is frozen** as an explicit cross-crate contract (the port-phasing doc's R6) so engine work parallelizes safely.

Items 1–4 and 6 are the *unknown-unknowns* gate — the defects a widget author cannot see. Items 5, 7, 8 are the *recurrence* gate — they stop the catalog from re-introducing the patterns. Item 9 is the parallelism gate.

**Not on the gate** (legitimately deferrable to overlap early catalog work): layer retained-rendering (D-7 — correct-but-slow), parallel-type renames (D-8 — loud), `BuildContext` `new_minimal` (D-9 — needed by Material, not widget #1), focus/tab-nav (D-10), all of P2. These are P1/P2 precisely because a *little* catalog survives them; they must close before the foundation is *declared stable*, not before it is *first built on*.

---

## 8. Closing assessment

**Is the foundation sound enough to build on?** After the P0 tier — **yes.** Before it — **no.**

The render *machine* is genuinely well-architected: the typestate `PipelineOwner`, lock-free `AtomicRenderFlags`, the arity system, Slab+ID-offset, the bounded crossbeam channel — these are gold-standard Rust, and every audit independently says "don't touch." That is the load-bearing 40%.

But the machine has **three stubbed phases** (layout invokes nothing, composite is a no-op, paint clears unpainted nodes) and **one wrong-algorithm core** (positional reconciliation with the keyed reconciler dead) — and these are exactly the **unknown-unknowns** (Ousterhout): they produce *demo-correct, production-broken* behavior. A widget catalog built today would build fine, render nothing laid-out, and lose all list state on reorder — and none of it would show until a real app. P0 is six concrete fixes (D-1 through D-4 are small, owner.rs-local; D-2 is bounded; D-5 is three design decisions) plus the gate (D-6 + the triggers). It is *small, well-understood work* — the cycle-4 audit already wrote skeletons for D-3 and D-4. It is small because the machine is sound; only the wiring of specific phases was never finished.

The deeper finding is the **eight systemic patterns**. The defects are not random — they are eight repeated *classes*, and a repeated class is a missing rule (the tactical-tornado diagnosis). FLUI already has the rule mechanism: `docs/PORT.md` refusal triggers. The single highest-leverage action in this entire plan is **installing the six new triggers (#8–#13)** — because that converts the answer from "we fixed the defects" (a snapshot, decays as 480k LOC of catalog is written) to "the defects cannot recur" (durable). Mythos has been fixing parallel-types and hygiene one cycle at a time and they reappear one cycle at a time; only a trigger ends that loop.

The most dangerous single defect: **D-2, index-not-key reconciliation.** Not because it is the hardest to fix — the correct reconciler is *already written* — but because it is the purest unknown-unknown on the most-traveled path. It is a `ListView` that loses your scroll position, a `Hero` that doesn't fly, a reordered list that resets — silently, in production, never in a test. The keyed reconciler sitting unused beside the positional loop is the workspace's tactical tornado made visible in one file.

**The Mythos-delta warning for the FOUNDATIONS doc:** Mythos is closing hygiene well, but **D-1, D-3, D-4 — the three stubbed render phases that block all of layout and composite — are audited but on no Wave's landing schedule.** They are not hard; they are *unowned*. The roadmap's Phase 0 ("close the Mythos cycles") as currently scoped would *not* close them, because cycle-4 already "closed" its audit by deleting the stubs subtractively. Phase 0 must be explicitly expanded to include D-1/D-3/D-4 as named deliverables, or the widget catalog starts on a layout phase that does nothing.

---

## Appendix — defect-to-audit traceability

| Defect | Primary audit source | Verified at HEAD |
|---|---|---|
| D-1 | `2026-05-22-flui-rendering-engine-audit.md` R-13 | yes — `owner.rs` comment confirms no prod `entry.layout()` |
| D-2 | `2026-05-22-flui-painting-view-audit.md` V-4; `architectural-contracts.md` Contract 5 | yes — `child_storage.rs:494` positional loop; `reconcile_children` 0 callers |
| D-3 | `2026-05-22-flui-rendering-engine-audit.md` R-4 | yes — `owner.rs:922` no-op + log string |
| D-4 | `2026-05-22-flui-rendering-engine-audit.md` R-15 | per audit |
| D-5a/b/c | `2026-05-22-architectural-contracts.md` Contracts 3/6/2/5 | per audit |
| D-6 | `2026-05-22-flui-rendering-engine-audit.md` R-1/R-2/R-3 | yes — only doc-mentions of `unimplemented!()` remain |
| D-7 | `2026-05-22-flui-layer-semantics-audit.md` | per audit |
| D-8 | `2026-05-22-flui-rendering-engine-audit.md` R-10; `2026-05-22-cycle4-wave2-design.md` | yes — `flui-engine/src/error.rs` still `pub enum RenderError` |
| D-9 | `architectural-contracts.md` Contract 4; `2026-05-22-flui-painting-view-audit.md` V-3 | yes — registry wired `build_owner.rs:651`; `new_minimal` hole open |
| D-10 | `2026-05-21-flui-interaction-audit-draft.md` / `-scheduler-audit.md` | per audit |
| D-11 | `2026-05-22-flui-foundation-tree-audit.md` T-3 | needs one verify (cycle-3 PR#103 may have closed) |
| D-12 | `2026-05-21-flui-scheduler-audit-draft.md` (un-executed draft) | per audit |
| SP-1..SP-8 | synthesized across all 14 audits | the eleven SP-1 instances spot-verified |
