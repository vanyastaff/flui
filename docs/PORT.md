[← Architecture](architecture.md) · [Back to README](../README.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Crates Map →](crates.md)

# Port Methodology

FLUI is a **port** of Flutter's three-tree architecture into Rust, not a redesign. This page is the working methodology for that port — the rules the maintainer refuses to break at write time, the per-crate documentation shape that records the port decisions, and the index that pins which crate holds which mapping.

PORT.md sits inside a four-document governance set:

1. [`STRATEGY.md`](../STRATEGY.md) — product strategy, the three port rules, "behavior loyal, structure Rust-native".
2. [`FOUNDATIONS.md`](FOUNDATIONS.md) — the architecture contract (target architecture, locked contracts, target crate graph).
3. **`PORT.md` (this page)** — the operational port methodology (refusal triggers, per-crate documentation template).
4. [`ROADMAP.md`](ROADMAP.md) — the construction plan (dependency-ordered phases from current to target).

For the rule-by-rule architectural guide (workspace layers, anti-pattern code examples, dependency DAG), read [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md). This page does not restate the strategy or contract layers; it is the operational layer that hangs off them.

---

## Refusal triggers

Each trigger is a rule the maintainer refuses to introduce. A violation found in review is the signal to either refactor the violating site or, if the same pattern is caught again, promote the trigger to a compile-time lint per the [Reactive lint promotion](#reactive-lint-promotion) rule below.

Triggers are seeded from observed friction in the workspace. Forward-looking triggers have zero current production violations and are enforced on introduction.

### 1. `RwLock` field on a type used inside `perform_layout` or `paint`

**Why:** the render hot path is strictly synchronous (see [`STRATEGY.md`](../STRATEGY.md) clause "sync hot path, async на краях"). A lock on a per-node storage type held across `perform_layout` or `paint` serialises the pipeline against itself and removes the "many readers OR one writer" guarantee the hot path depends on. Shared infrastructure locks (`PipelineOwner`, `WidgetsBinding`, route plumbing) are different — they sit one level above per-node mutation and are covered in [Lock decisions](#lock-decisions).

**Back-references:** [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) v2.2.0 Anti-Patterns ("`Arc<Mutex<>>` for tree structures"); [`STRATEGY.md`](../STRATEGY.md) "sync hot path".

**Regex (used by `just port-check`):** `RwLock<\s*Box<\s*dyn\s+(RenderObject|Layer\b|ContainerLayer)` (storage-shaped violations). Scope extended in Mythos Step 13 of the `flui-layer` chain to cover `crates/flui-layer/src/` and to match `dyn Layer` / `dyn ContainerLayer` shapes as well. Re-confirmed in Mythos Step 13 of the `flui-painting` chain to cover the post-split `crates/flui-painting/src/` subdirectories (`canvas/`, `display_list/`, `text_layout/`, `text_painter/`) as a forward-looking guard.

### 2. `Box<dyn RenderObject<_>>` wrapped in any interior-mutability primitive in render storage

**Why:** owned `Box<dyn RenderObject<_>>` stored as a plain field is acceptable — it is the chosen post-U2 baseline that preserves the open-set trait (blanket `impl<T: RenderBox + Diagnosticable> RenderObject<P> for T`) while delegating mutation discipline to the borrow checker through `&mut RenderTree`. The actual hazard is **wrapping** the trait object in any interior-mutability primitive (`RwLock`, `Mutex`, `RefCell`, `Cell`, `UnsafeCell`) on the storage type, because that would re-introduce the lock-or-interior-mutability problem the U2 refactor removed (the canonical violation was `RwLock<Box<dyn RenderObject<P>>>`).

Trigger 1 catches the specific `RwLock` variant. Trigger 2 catches any other interior-mutability wrap that would smuggle the same problem back in under a different primitive.

The *funnel* signatures (`tree.rs::insert_box`, view → render `From` impls) accept `Box<dyn RenderObject<_>>` as a transient parameter type and are not the target — the violation is the stored-and-wrapped shape.

**Back-references:** [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) example "`RenderBad { children: Vec<Box<dyn RenderObject>> }` — forbidden"; [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) Principle IV.

**Regex:** `(RwLock|Mutex|RefCell|Cell|UnsafeCell)<\s*Box<\s*dyn\s+(RenderObject|Layer\b|ContainerLayer)` constrained to render-storage modules, `crates/flui-layer/src/`, and `crates/flui-painting/src/`. Scope and trait-name set extended in Mythos Step 13 of the `flui-layer` chain; re-confirmed for the post-split `flui-painting` subdirectories in Mythos Step 13 of the `flui-painting` chain.

### 3. `async fn` on `View::build`, `RenderObject::layout`, `RenderObject::paint`

**Why:** the same sync-hot-path clause. Async on these methods would force the scheduler to await within a frame budget critical path.

**Back-references:** [`STRATEGY.md`](../STRATEGY.md) "sync hot path, async на краях"; permitted at IO (`flui-assets`), scheduler (`flui-scheduler`), build pipeline (`flui-build`) only.

**Regex:** `async\s+fn\s+(build|layout|paint|perform_layout|composite|render|fire_composition_callbacks)\b` constrained to `crates/flui-{rendering,view,painting,layer}/src/**`. Scope and verb set extended in Mythos Step 13 of the `flui-layer` chain to catch layer-level async (`composite`, `render`, `fire_composition_callbacks`). Re-confirmed in Mythos Step 13 of the `flui-painting` chain to recurse into the post-split `crates/flui-painting/src/` subdirectories (rg recurses naturally; verified via `bash scripts/port-check.sh -v`).

**Whitelist:** `crates/flui-view/src/binding.rs` route-notification handlers (`handle_pop_route`, `handle_push_route`, `handle_commit_back_gesture`, `handle_request_app_exit`) are async per Flutter's `SystemChannels` callback shape; they sit on the binding layer, not the render path.

### 4. `Mutex` on dirty-list state mutated during the build / layout / paint cycle 🔮

**Forward-looking** — no current production violation. The existing dirty tracking at `crates/flui-rendering/src/storage/state.rs` uses `AtomicRenderFlags` + `OnceCell` + `AtomicOffset` ("10x faster than RwLock" per the module docstring). The trigger guards against regression.

**Why:** dirty-list state is touched per-frame; a mutex would serialise frame producers and consumers needlessly. Lock-free atomics are the in-crate precedent.

**Regex:** `Mutex<\s*Vec<\s*ElementId\b|Mutex<\s*HashSet<\s*ElementId\b|Mutex<\s*HashMap<\s*ElementId\b` constrained to `crates/flui-rendering/src/**` excluding `#[cfg(test)]` modules.

### 5. `Arc::clone` performed inside the per-frame paint loop on a per-render-object basis 🔮

**Forward-looking** — no current production violation. The per-frame paint loop at `crates/flui-rendering/src/pipeline/owner.rs` does not perform `Arc::clone`.

**Why:** per-frame allocations are the largest controllable frame-budget tax. `Arc::clone` is cheap individually but compounds across hundreds of render objects times 60 frames per second. Caller is asked to pass `&Arc<T>` or `&T` rather than clone.

**Regex:** `Arc::clone\(` constrained to `crates/flui-rendering/src/objects/**/*.rs` and `crates/flui-engine/src/wgpu/layer_render.rs` (the per-layer wgpu walk; scope extended in Mythos Step 13 of the `flui-layer` chain as a forward-looking guard).

### 6. Recursive `Box<dyn View>` stored in element child collections

**Why:** the unified element reconciler is built around generic dispatch (`Element<V, A, B>`); storing user-defined `Box<dyn View>` in child storage forces a runtime-typed boundary into the reconciliation hot path. Funnel parameters that accept `Box<dyn View>` at the boundary are acceptable; storing them as children is not.

**Back-references:** [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) Principle IV ("Prefer generics and enum dispatch over `dyn` trait objects").

**Regex:** `:\s*Vec<\s*Box<\s*dyn\s+View|:\s*Box<\s*dyn\s+View` constrained to `crates/flui-view/src/element/child_storage.rs` and storage struct definitions in `crates/flui-view/src/element/**`.

### 7. `Arc<Mutex<*Renderer | *Pool | wgpu::*>>` field in `flui-engine` wgpu module 🔮

**Forward-looking** — added in Mythos Step 9 of the `flui-engine` chain. Catches regressions of the `Arc<parking_lot::Mutex<OffscreenRenderer>>` and `Arc<Mutex<TexturePoolInner>>` shapes documented as Outstanding refactors in [`crates/flui-engine/ARCHITECTURE.md`](../crates/flui-engine/ARCHITECTURE.md). Today's known sites are excluded via file-glob (`!**/texture_pool.rs`, `!**/renderer.rs`, `!**/backend.rs`) so the trigger reports clean post-chain; when the corresponding Outstanding refactor lands, the file-glob exclusions go away.

**Why:** the wgpu single-mutator runtime invariant means `Arc<Mutex<T>>` on engine subsystems hides a single-thread access pattern behind shared-mutability ceremony. The lock is uncontended in production but the shape mismatches the type-level invariant; a future regression would re-introduce the same maintenance burden.

**Back-references:** verdict §12 rejected design #2 (`Arc<RwLock<Renderer>>` shared); strategy clause "single owner of wgpu resources."

**Regex:** `^\s+(pub\s+)?\w+\s*:\s*(Option<\s*)?Arc<\s*(parking_lot::)?(Mutex|RwLock)<\s*((super::)?(\w+::)*\w*(Renderer|Pool)\w*|wgpu::\w+)` constrained to `crates/flui-engine/src/wgpu/`, with file-glob exclusions for the three Friction-log-tracked sites listed above. Anchored to struct-field syntax (leading whitespace + optional `pub` + ident + `:`); inner alternation `((super::)?(\w+::)*\w*(Renderer|Pool)\w*|wgpu::\w+)` is grouped so `wgpu::*` matches only at the outer-type position. Catches both `Arc<...>` and `Option<Arc<...>>` field shapes. Tightened after Copilot review on PR #79.

### 8. `unimplemented!()` / `todo!()` in production `fn` body

**SP-1 — stubbed-but-called.** A function whose body panics on entry is an API that publishes a contract without honoring it. The trigger fires whenever `unimplemented!(` or `todo!(` appears outside test code.

**Allowlist marker:** `// PORT-CHECK-OK-STUB: <reason + tracking-issue>` on the same line as the panic. The reason should name the tracking issue or follow-up doc so the stub doesn't become permanent.

**Scope:** framework crates (`crates/`), excluding tests (`tests/`, `test*.rs`), examples, and the per-platform stub modules (`crates/flui-platform/src/platforms/{linux,ios,android}/`) which are tracked outside SP-1 under the platform-impl track in [`ROADMAP.md`](ROADMAP.md).

**Regex:** `unimplemented!\s*\(|todo!\s*\(` with doc-comment and marker filters.

**Back-references:** [architecture-correction-plan §SP-1](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U41](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### 10. Parallel cross-crate type definitions

**SP-3 — same identifier `pub struct` / `pub enum` / `pub trait` defined in 2+ distinct framework crates.** Either the same concept is implemented twice (consolidate) or two unrelated concepts collide on a single name (rename one).

Re-exports (`pub use foo::Bar`) do not trip the trigger — only literal `pub <kind> <Name>` declarations are counted.

**Allowlist marker:** `// PORT-CHECK-OK-SP3: <reason + tracking-issue>` on the same line as the `pub <kind> <Name>` declaration. Pre-existing parallel definitions in the current codebase are individually marked so future ADDITIONS are caught; the marker reason should point to a consolidation tracking issue.

**Scope:** framework crates (`crates/`), excluding tests + examples.

**Regex:** `pub +(struct|enum|trait) +[A-Z][a-zA-Z0-9_]*` with crate-attribution via path, then duplicate detection across distinct crates.

**Back-references:** [architecture-correction-plan §SP-3](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U42](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### 11. Speculative scaffolding: `pub mod` with zero workspace consumers

**SP-4 — `pub mod <name>;` declared in `lib.rs` that is (a) not behind `#[cfg(feature = "unstable-*")]`, (b) not re-exported via `pub use [crate::]<name>::` in the same `lib.rs`, AND (c) not referenced as `<crate>::<name>` anywhere in the workspace outside the defining crate.** This catches speculative `pub mod` surfaces that publish API without consumers.

**Allowlist marker:** `// PORT-CHECK-OK-SP4: <reason + tracking-issue>` on the same line as the `pub mod` declaration. Common reasons: macro export bypass (`#[macro_export]` items consumed via macro invocation not module path), future-consumer binding entry, intentional API surface for downstream integrators.

**Limitations:** mechanical scan — catches lib.rs-level `pub mod`, NOT sub-module speculation (`mod foo { pub mod bar; }`). For deeper SP-4 audits see the manual verdicts in [architecture-correction-plan §SP-4](research/2026-05-22-architecture-correction-plan.md).

**Back-references:** [architecture-correction-plan §SP-4](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U43](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md), `flui-tree-unified-interface-intent` memory.

### 12. Lock placement in public API

**SP-6 — `RwLock` / `Mutex` / `Arc<RwLock<...>>` in a `pub fn` return type OR a `pub` field of a trait/struct.** Lock types leak the framework's concurrency model across module boundaries; every caller has to reason about lock ordering / poisoning / re-entrancy. SP-6's verdict is that locks should live behind private fields; public APIs should expose immutable snapshots or scoped callbacks.

**Patterns flagged:**
* `pub fn foo() -> RwLockReadGuard<...>` / `RwLockWriteGuard<...>` / `MutexGuard<...>` / `RwLock<...>` / `Mutex<...>`
* `pub field: (Arc<)?(parking_lot::)?(RwLock|Mutex)<...>`

**Allowlist marker:** `// PORT-CHECK-OK-SP6: <reason + tracking-issue>` on the same line as the declaration. Pre-existing leaks in the binding / context / callback-storage layers are marked individually; the marker reason should point to the consolidation tracking issue.

**Back-references:** [architecture-correction-plan §SP-6](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U44](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### 13. Constructor-time panics

**SP-8 — `unwrap()` / `expect(` / `panic!(` / `assert!(` inside a public CONSTRUCTOR body (`pub fn new` / `pub fn from_*` / `pub fn try_*`).** Turns argument-validation bugs into process aborts at the public API surface. The SP-8 verdict is that public constructors should return `Result` or take pre-validated types.

**Allowed:** `debug_assert!` (compiled out in release).

**Mechanical scope (deliberately narrow — high precision, accepts false-negatives):** single-line constructor bodies of the shape `pub fn new(...) -> Self { ... .unwrap()/.expect()/panic!/assert! ... }` (inline body with one of the panic forms on the SAME line as the `pub fn (new|from_*|try_*)` signature). Multi-line constructor bodies are NOT inspected; rustc + clippy lints (`clippy::expect_used`, `clippy::unwrap_used`) cover that surface where opted in.

**Allowlist marker:** `// PORT-CHECK-OK-SP8: <reason>` on the same line as the panic.

**Back-references:** [architecture-correction-plan §SP-8](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U45](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### Reactive lint promotion

Triggers grow reactively. A new trigger is added to this list when an anti-pattern is caught in review; it does not pre-exist its first observation.

A trigger is promoted from a doc entry to a clippy lint **only after the same pattern has been caught at least twice in review**. The first-promotion mechanism is a `[workspace.lints.clippy]` deny entry in the root [`Cargo.toml`](../Cargo.toml). `dylint` (custom plugin in a dedicated `crates/flui-lints/`) and `cargo-deny[bans]` (dependency-level rules) stay deferred — they are heavier than the first promotion warrants.

If a future trigger's shape cannot be expressed in any clippy lint that exists (e.g., field type + use-site predicate), the trigger remains in this document plus the `just port-check` grep and promotion is deferred until the toolchain catches up. This is acceptable — the grep is the durable enforcement layer.

---

## Lock decisions

The workspace contains ~62 `RwLock` sites at the time of this writing. Most are allowed; one was the canonical violation that motivated this methodology. The categorisation below resolves the "shared infrastructure vs per-node storage" line.

| Site | Category | Disposition |
| --- | --- | --- |
| `RenderEntry.render_object` (`crates/flui-rendering/src/storage/entry.rs:46`) — per-node storage, locked inside `layout()` | Per-node storage / in-loop mutation | **Forbidden** (Trigger 1, 2) — exemplar refactor target. |
| `PipelineOwner` parents and back-references (`crates/flui-rendering/src/pipeline/owner.rs`, `crates/flui-rendering/src/storage/tree.rs`, `crates/flui-view/src/view/root.rs`, `crates/flui-view/src/binding.rs`, `crates/flui-rendering/src/view/render_view.rs`) | Shared infrastructure / setup-time | Allowed — soundness-rewrite precedent ([`docs/plans/2026-03-31-core-crates-hardening.md`](plans/2026-03-31-core-crates-hardening.md) Task 7 explicitly replaced a raw pointer with `Weak<RwLock<PipelineOwner>>` to remove `unsafe impl Send/Sync` markers). |
| `ViewportOffset` listener lists (`crates/flui-rendering/src/view/viewport_offset.rs:138, 262`) | Listener registry, not on layout/paint | Allowed. |
| `BuildContext` tree/owner refs (`crates/flui-view/src/context/element_build_context.rs:47-505`) — `Arc<RwLock<ElementTree>>`, `Arc<RwLock<BuildOwner>>` | Build phase, not layout/paint | Allowed; flagged as latent friction in the `flui-view` `Friction log` (out of this methodology's first pass). |
| `MouseTracker` maps (`crates/flui-rendering/src/input/mouse_tracker.rs:294-303`) | Tracker state, not on layout/paint | Allowed. |
| `static ERROR_VIEW_BUILDER: RwLock<Option<...>>` (`crates/flui-view/src/view/error.rs:40`) | Process-wide singleton | Allowed. |
| Image cache + listener locks (`crates/flui-painting/src/binding.rs:49, 61, 331`) | Off the recording hot path; `DisplayList` recording is single-threaded `Send` | Allowed. |
| `FocusManager.traversal_policy: RwLock<Box<dyn FocusTraversalPolicy>>` (`crates/flui-interaction/**`) | Off-hot-path policy plug | Allowed. |
| `GestureArena.entries: Mutex<HashMap<...>>` | Write-heavy, off render hot path | Allowed. |

The general rule is: **a lock that protects shared infrastructure mutated outside the per-frame `perform_layout`/`paint` window is allowed; a lock that protects per-node storage or state touched inside that window is forbidden.**

---

## Mapping rules

These are the rules the methodology uses to resolve Dart ↔ Rust translation conflicts at port time. They are operational summaries of the strategy clauses in [`STRATEGY.md`](../STRATEGY.md) — when a clause conflicts with a refactor proposal, the clause wins, and the proposal is reshaped.

### Flutter behaviour primacy, with binding-deletion carve-out

Algorithms (`build` / `layout` / `paint`, lifecycle FSM, dependency tracking, child reconciliation through keys) are ported 1:1 from `.flutter/`. Conflicts with Rust-idiomatic alternatives resolve in favour of Flutter semantics. This means:

- Element lifecycle FSM stays even when a typestate-only sealed enum would be "cleaner".
- Mixin → trait + `ambassador` delegation is the translation; not a typestate or generic-only restructure.
- `RenderObject::parent_data` indirection stays even when an arity-keyed enum would compile-time-eliminate the indirection.

**Carve-out:** a Flutter binding may be **deleted**, not ported, when a Rust-native crate stack already owns the responsibility end-to-end. The canonical precedent is the removal of `PlatformTextSystem` in [`docs/plans/2026-03-31-platform-roadmap.md`](plans/2026-03-31-platform-roadmap.md) Task 1 — cosmic-text + glyphon + flui-assets covers the text-shaping responsibility, so the Flutter abstraction was removed rather than re-implemented. The carve-out applies when:

- A Rust-native crate (or short crate stack) end-to-end owns the responsibility, not just a dependency of it.
- The deletion does not break observable Flutter semantics that downstream code depends on.
- The decision is recorded in the affected crate's `ARCHITECTURE.md` `## Mapping decisions` section with the precedent citation.

### Compile-time over runtime

Where a runtime check and a compile-time check express the same constraint, the compile-time form is required. Concretely:

- Arity types (`Leaf` / `Single` / `Optional` / `Variable`) over runtime child-count assertions.
- Typestate builders (e.g., `BuilderContextBuilder<P, Pr>` in `flui-build`) over runtime config validation.
- Sealed traits (e.g., `PlatformBuilder`) over open-world dispatch.

`TypeId` lookup for `InheritedView` ancestry is the single allowed runtime-reflection window per the strategy clause.

### Sync hot path, async at edges

`async fn` is forbidden in the render hot path: `View::build`, `RenderObject::layout`, `RenderObject::paint`, `RenderObject::perform_layout`, and their helpers. Permitted at IO boundaries in `flui-assets`, the scheduler in `flui-scheduler`, the build pipeline in `flui-build`, and route-notification handlers in `flui-view/src/binding.rs` (which sit on the binding layer, not the render path).

### Multi-source references in `## Mapping decisions`

The per-crate `ARCHITECTURE.md` `## Mapping decisions` section may cite any audited reference codebase, not Flutter alone. The workspace already routinely cites Flutter, GPUI, Iced, Makepad, Vello, and Skia as design references (see [`docs/plans/2026-03-31-engine-hardening.md`](plans/2026-03-31-engine-hardening.md)). The Flutter-primacy rule above is about *semantics*, not source exclusivity — the structural shape may be drawn from any of the audited references when their pattern fits Rust idioms better.

---

## Per-crate `ARCHITECTURE.md` template

Each active crate that participates in the port carries a root-level `crates/<crate>/ARCHITECTURE.md` (per [`AGENTS.md`](../AGENTS.md) naming convention). The file follows this template. Crates adopt the template incrementally as a port or refactor touches them — no big-bang sweep.

### Fixed sections (all five required)

1. **`## Flutter source mapping`** — Dart-source-to-Rust-source correspondence. May be a table (file → file), a hierarchy reference (linking out to an appendix like `crates/flui-rendering/flutter-rendering-hierarchy.md`), or a narrative walk. The goal is that a reader can find the Dart origin of any Rust type in the crate.
2. **`## Mapping decisions`** — places where the Rust shape diverges from the Dart shape and the rationale. Each entry names the conflict, the choice, and the reference (a strategy clause, a refusal trigger, or a precedent plan). When an entry is an exception to a refusal trigger, use the "Accepted trade-offs" format from [`docs/plans/2026-03-31-custom-render-callback-design.md`](plans/2026-03-31-custom-render-callback-design.md) — state the rule, the justification, the alternatives considered, the trade-off accepted.
3. **`## Thread safety`** — table of `RwLock` / `Mutex` / atomic primitives used in the crate with kind + rationale + on-hot-path/off-hot-path classification. Lifted from [`crates/flui-interaction/docs/ARCHITECTURE.md`](../crates/flui-interaction/docs/ARCHITECTURE.md) precedent. An empty table is acceptable for crates with no shared mutable state — an explicit "no locks" declaration is itself useful documentation.
4. **`## Friction log`** — known shape issues present in the current code that violate a refusal trigger or a strategy clause but have not been refactored yet. Each entry names the site (file:line), the violated rule, and what would need to change.
5. **`## Outstanding refactors`** — planned cleanups with file:line references and enough scope detail that a fresh `/aif-implement` dispatch can pick one up without out-of-band clarification. The `Friction log` describes what is broken now; `Outstanding refactors` describes what to fix next.

### Optional sections

Add per crate when warranted:

- **`## Test parity notes`** — places where the crate's test suite intentionally diverges from Flutter's test surface.
- **`## Exception ledger`** — when the crate accumulates more than one justified exception to a refusal trigger, consolidate the "Accepted trade-offs" entries here.

### Graft instructions for existing docs

Three crates already hold port-flavoured documents that predate this template:

- [`crates/flui-foundation/ARCHITECTURE.md`](../crates/flui-foundation/ARCHITECTURE.md) — Flutter-walk + Architecture Decision Summary; graft by appending the missing `## Thread safety` / `## Friction log` / `## Outstanding refactors` sections; do not rewrite the existing body.
- [`crates/flui-rendering/flutter-rendering-hierarchy.md`](../crates/flui-rendering/flutter-rendering-hierarchy.md) — 1352-LOC Flutter class hierarchy dump; remains as a sibling appendix linked from the templated `crates/flui-rendering/ARCHITECTURE.md`.
- [`crates/flui-view/UNIFIED_ELEMENT.md`](../crates/flui-view/UNIFIED_ELEMENT.md) — element behaviour taxonomy; remains as a sibling appendix linked from the future templated `crates/flui-view/ARCHITECTURE.md` when that crate is templated.

Four other crates carry `crates/<crate>/docs/ARCHITECTURE.md` files (`flui-painting`, `flui-interaction`, plus disabled crates `flui-animation`, `flui-assets`). Relocating those to the crate root per the `AGENTS.md` convention is deferred to a follow-up doc-tidying PR; this methodology does not require relocation upfront.

---

## Index

| Crate | `ARCHITECTURE.md` state | Status |
| --- | --- | --- |
| [`flui-foundation`](../crates/flui-foundation/ARCHITECTURE.md) | Templated (grafted 2026-05-19) | Active |
| [`flui-rendering`](../crates/flui-rendering/ARCHITECTURE.md) | Templated 2026-05-20 (exemplar instance, U2 refactor recorded) | Active |
| `flui-types` | Not yet templated | Active |
| `flui-tree` | Not yet templated | Active |
| `flui-platform` | Not yet templated | Active |
| [`flui-painting`](../crates/flui-painting/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active |
| `flui-semantics` | Not yet templated | Active |
| `flui-scheduler` | Not yet templated | Active |
| [`flui-layer`](../crates/flui-layer/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active |
| `flui-interaction` | `crates/flui-interaction/docs/ARCHITECTURE.md` (pre-template; precedent for `## Thread safety` format) | Active |
| [`flui-engine`](../crates/flui-engine/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active |
| `flui-log` | Not yet templated | Active |
| `flui-hot-reload` | Not yet templated | Active |
| `flui-view` | `crates/flui-view/UNIFIED_ELEMENT.md` (companion; not templated) | Active |
| `flui-app` | Not yet templated | Active |
| `flui-animation` | `crates/flui-animation/docs/ARCHITECTURE.md` (pre-template) | Disabled |
| `flui-reactivity` | Not yet templated | Disabled |
| `flui-devtools` | Not yet templated | Disabled |
| `flui-cli` | Not yet templated | Disabled |
| `flui-build` | Not yet templated | Disabled |
| `flui-assets` | `crates/flui-assets/docs/ARCHITECTURE.md` (pre-template) | Disabled |

Authoritative workspace state lives in [`AGENTS.md`](../AGENTS.md) and [`docs/crates.md`](crates.md); this index restates "templated yes/no" only.

External references this methodology builds on:

- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — v2.2.0 anti-patterns and architectural rules.
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full anti-pattern list with code examples.
- [`STRATEGY.md`](../STRATEGY.md) — port rationale, Bun precedent, three architectural clauses.
- [`docs/plans/2026-03-31-core-crates-hardening.md`](plans/2026-03-31-core-crates-hardening.md) — `Weak<RwLock<PipelineOwner>>` precedent.
- [`docs/plans/2026-03-31-platform-roadmap.md`](plans/2026-03-31-platform-roadmap.md) — `PlatformTextSystem` deletion precedent (source of the binding-deletion carve-out).
- [`docs/plans/2026-03-31-custom-render-callback-design.md`](plans/2026-03-31-custom-render-callback-design.md) — canonical justified `Box<dyn>` exception template.
- [`docs/plans/2026-03-31-engine-hardening.md`](plans/2026-03-31-engine-hardening.md) — multi-source reference precedent (GPUI, Makepad, Iced, Vello, Skia).

---

## Verification

The `just port-check` recipe runs the refusal-trigger regexes from this document against the workspace and exits non-zero on any match outside the whitelist. Run it before opening a PR that touches `flui-rendering`, `flui-view`, or `flui-painting`:

```text
just port-check               # silent on pass; lists each violation on fail
just port-check-verbose       # prints "ok" lines for each passing trigger
```

The underlying script lives at [`scripts/port-check.sh`](../scripts/port-check.sh). It runs seven `rg` (ripgrep) invocations — one per refusal trigger — and filters out doc-comment matches. The regexes are derived directly from the trigger entries in this document; when a trigger changes here, the script changes too.

**Cross-platform note.** The script is bash. On Windows, run via Git Bash or WSL — both ship with `bash` and modern `rg` on PATH. A PowerShell sibling is not provided in this iteration because the regex set is identical and dual-maintenance is not warranted at solo-maintainer scale.

The recipe is not part of `just ci` by default — refusal triggers are a write-time guard, and CI carries the lint baseline plus the test suite. When a trigger is promoted to a clippy lint per the [Reactive lint promotion](#reactive-lint-promotion) rule, that lint runs under `cargo clippy --workspace -- -D warnings` (already in `just ci`); the doc entry stays as the human-readable surface for the rule.

### Self-test (negative-test confirmation)

`port-check` itself can be exercised by introducing a deliberate violation in a scratch file (e.g., `crates/flui-rendering/src/__port_check_scratch.rs` with a `RwLock<Box<dyn RenderObject<P>>>` field) and confirming the recipe exits non-zero, names the file and line, and references the correct trigger ID. Delete the scratch file when done. This confirms the recipe still distinguishes real violations from whitelist entries; do it after any change to the trigger regexes or the whitelist file globs.

---

[← Architecture](architecture.md) · [Back to README](../README.md) · [Crates Map →](crates.md)
