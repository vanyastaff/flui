[← Architecture](architecture.md) · [Back to README](../README.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Crates Map →](crates.md)

# Port Methodology

FLUI is a **port** of Flutter's three-tree architecture into Rust, not a redesign. This page is the working methodology for that port. It plays two roles in one document:

1. **Governance layer** — the rules the maintainer refuses to break at write time (refusal triggers), the lock-decision matrix, the per-crate documentation shape that records port decisions, and the index of which crate holds which mapping.
2. **Operational translation manual** — the concrete Dart→Rust type map, idiom map, string discipline, error-shape canon, marker tier, and ecosystem-adoption table that turn a Dart file into a Rust file without ad-hoc per-unit re-derivation.

PORT.md sits inside a four-document governance set:

1. [`STRATEGY.md`](../STRATEGY.md) — product strategy, the three port rules, "behavior loyal, structure Rust-native".
2. [`FOUNDATIONS.md`](FOUNDATIONS.md) — the architecture contract (target architecture, locked contracts, target crate graph).
3. **`PORT.md` (this page)** — governance + operational translation manual.
4. [`ROADMAP.md`](ROADMAP.md) — the construction plan (dependency-ordered phases from current to target).

For the rule-by-rule architectural guide (workspace layers, anti-pattern code examples, dependency DAG), read [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md). This page does not restate the strategy or contract layers; it is the operational layer that hangs off them.

The translation manual draws inspiration from Bun's [oven-sh/bun#PORTING.md](https://github.com/oven-sh/bun/blob/main/docs/PORTING.md), with one principled inversion: **flui actively embraces the Rust ecosystem**. Where Bun bans `tokio`/`rayon`/`hyper`/`futures` and rolls its own primitives, flui adopts mature ecosystem crates (`parking_lot`, `dashmap`, `smallvec`, `ambassador`, `bon`, `thiserror`, `tracing`, `wgpu`, `moka`, `tokio` LTS). See [§Ecosystem-first principle](#ecosystem-first-principle) for the adoption table and the version policy.

## Contents

**Governance layer** (write-time refusal rules, lock-decision matrix, per-crate documentation shape):

- [§Refusal triggers](#refusal-triggers) — 20 anti-patterns the maintainer refuses to introduce, with grep regexes
- [§Lock decisions](#lock-decisions) — allowed vs forbidden `RwLock`/`Mutex` placements
- [§Mapping rules](#mapping-rules) — Flutter behaviour primacy + binding-deletion carve-out + compile-time-over-runtime + sync-hot-path
- [§Per-crate `ARCHITECTURE.md` template](#per-crate-architecturemd-template) — required and optional sections per crate
- [§Index](#index) — which crate carries which template state
- [§Verification](#verification) — `just port-check` recipe + self-test

**Translation manual** (operational Dart→Rust conversion guidance):

- [§Dart → Rust type map](#dart--rust-type-map) — primitives, nullability, Flutter framework types, sentinel/error types
- [§Dart → Rust idiom map](#dart--rust-idiom-map) — control flow, optional/null, closures, object model, cfg, iteration, concurrency, atomics, mixin worked example
- [§Strings discipline](#strings-discipline) — 8-row decision tree (UI text / message fields / static IDs / Cow / interned / external bytes / paths / widget keys)
- [§Error mapping canonical shape](#error-mapping-canonical-shape) — `thiserror` + `#[non_exhaustive]` + `Box<str>` + `anyhow` at app-edge only
- [§Inline port markers tier](#inline-port-markers-tier) — `TODO(port)` / `PERF(port)` / `PORT NOTE` / `SAFETY` grammar + `port-check.sh` integration
- [§Ecosystem-first principle](#ecosystem-first-principle) — adopted-crates table + version policy + Rust 1.95/1.96 stabilizations folded into the port
- [§Don't translate](#dont-translate) — source-level dropped + binding-deletion precedents

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

**Regex:** `Mutex<\s*(Vec|HashSet|HashMap|BTreeSet|BTreeMap)<\s*ElementId` constrained to `crates/flui-rendering/src/**` excluding `#[cfg(test)]` modules and `**/state.rs` (which hosts the `MockTree` test fixture). The collection-type set is unified with the script (`scripts/port-check.sh` trigger 4) — both ordered and unordered map/set forms catch a regression.

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

### 9. Sanctioned `dyn`-boundary registry (FR-036)

**FR-036 — every `Box<dyn …>` / `&dyn …` / `Arc<dyn …>` / `Rc<dyn …>` introduction (and every type alias of that shape) in the framework crates must either (a) name a sanctioned trait from the inline allowlist, (b) match a language-runtime exempt pattern (`Pin<Box<dyn Future>>`, `Box<dyn Iterator>`, `&dyn Fn*` callback parameters), or (c) carry an explicit `// PORT-CHECK-OK-DYN:` marker on the same line.** Phase 3.1 §U30 of the view/element core-contracts plan installs this trigger as the canonical FR-036 enforcement layer.

**Allowlist marker:** `// PORT-CHECK-OK-DYN: <one-line justification>` on the same line as the `dyn`-introducing declaration. Multi-line declarations either keep the marker on the `Box<` line (matched by the scan) or refactor to a type alias that fits one line + carries its own marker.

**Sanctioned trait allowlist** (categories per FR-029 #1-#5 + pre-existing framework surfaces): element-storage sub-traits (`ElementBase` / `ElementBehavior` / `StatelessElementBase` / `StatefulElementBase` / `ProxyElementBase` / `InheritedElementBase` / `RenderElementBase` / `RootElementBase` / `ErrorElementBase`), BoxedView (`View` / `BoxedView` / `ViewObject`), pipeline-owner type-erasure (`Any`), error / observer / animation / owned-callback chains (`Error` / `Listenable` / `Animation` / `WidgetsBindingObserver` / `Fn` / `FnMut` / `FnOnce`), protocol-layout erasure (`BoxLayoutCtxErased` / `SliverLayoutCtxErased` — D-block PR-A1b §U19 / memo D5), and pre-existing surfaces (`ViewKey` / `BuildContext` / `Notification` / `NotifiableElement` / `RenderObject` / `RenderObjectTrait`). Add a trait here when its `dyn` usage is widespread enough that per-site markers become noise; remove only after auditing that the trait's `dyn` surface is genuinely gone.

**Scope:** framework crates (`crates/flui-view/src`, `crates/flui-foundation/src`, `crates/flui-tree/src`, `crates/flui-engine/src`, `crates/flui-rendering/src`, `crates/flui-interaction/src`).

**Multi-line declaration handling:** the scan does NOT use `rg -U` multiline mode (mixing multi-line output blocks with line-oriented `grep -Ev` filters partial-filters multi-line matches → false positives and silent bypasses). The single-line scan catches rustfmt-formatted code (which collapses `Box<dyn Trait>` to one line whenever possible).

**Back-references:** [specs/004-view-element-core/spec.md FR-036](../specs/004-view-element-core/spec.md), [Phase 3.1 §U30](plans/2026-05-22-005-feat-view-element-core-contracts-plan.md).

### 10. Parallel cross-crate type definitions

**SP-3 — same identifier `pub struct` / `pub enum` / `pub trait` defined in 2+ distinct framework crates.** Either the same concept is implemented twice (consolidate) or two unrelated concepts collide on a single name (rename one).

Re-exports (`pub use foo::Bar`) do not trip the trigger — only literal `pub <kind> <Name>` declarations are counted.

**Allowlist marker:** `// PORT-CHECK-OK-SP3: <reason + tracking-issue>`. The scan checks a ±2-line window around the `pub <kind> <Name>` declaration (preceding line + same line + 2 lines after), so the marker survives rustfmt moving a trailing same-line comment on block-opening decls (`pub enum Foo {`, `pub struct Bar {`) into the body as the first non-blank line. Place the marker on the same line for one-liner decls (`pub struct Foo(pub u32);`) or on the preceding line for block decls; both forms are accepted.

**Scope:** framework crates (`crates/`), excluding tests + examples.

**Regex:** `pub +(struct|enum|trait) +[A-Z][a-zA-Z0-9_]*` with crate-attribution via path (backslash-normalized for Windows portability), then duplicate detection across distinct crates.

**Back-references:** [architecture-correction-plan §SP-3](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U42](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### Cross.H2. Canonical homes for historical parallel-type collapses

**The three D-8 collisions must stay collapsed.** Trigger 10 catches arbitrary same-name duplicate `pub struct` / `pub enum` / `pub trait` definitions, but D-8 had three concrete historical seams that downstream code depends on staying canonical:

- `ViewKey` trait: canonical home is `flui-foundation`.
- `IndexedSlot` struct: canonical home is `flui-tree`.
- `TargetPlatform` enum: canonical home is `flui-types`.

**Scope:** `crates/`, Rust source only.

**Allowlist:** none. Re-export the canonical type if ergonomics require it; do not define a second local copy.

**Back-references:** [framework spine repair plan §U1-U3](plans/2026-05-21-002-feat-framework-spine-repair-plan.md), `docs/ROADMAP-TRACKER.md` `H2`.

### 11. Speculative scaffolding: `pub mod` with zero workspace consumers

**SP-4 — `pub mod <name>;` declared in `lib.rs` that is (a) not behind `#[cfg(feature = "unstable-*")]` on its preceding non-blank line, (b) not re-exported via `pub use [crate::]<name>::` in the same `lib.rs`, AND (c) not referenced as `<crate>::<name>` anywhere in the workspace outside the defining crate.** This catches speculative `pub mod` surfaces that publish API without consumers.

**Allowlist marker:** `// PORT-CHECK-OK-SP4: <reason + tracking-issue>` on the same line as the `pub mod` declaration. Common reasons: macro export bypass (`#[macro_export]` items consumed via macro invocation not module path), future-consumer binding entry, intentional API surface for downstream integrators.

**Limitations:** mechanical scan — catches lib.rs-level `pub mod`, NOT sub-module speculation (`mod foo { pub mod bar; }`). For deeper SP-4 audits see the manual verdicts in [architecture-correction-plan §SP-4](research/2026-05-22-architecture-correction-plan.md).

**Back-references:** [architecture-correction-plan §SP-4](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U43](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md), [view-tree-foundation audit "Post-audit correction"](research/2026-05-21-view-tree-foundation-audit.md) (zero-consumer flui-tree surface is deliberate unified-tree infrastructure, not a deletion signal).

### 12. Lock placement in public API

**SP-6 — `RwLock` / `Mutex` / `Arc<RwLock<...>>` in a `pub fn` return type OR a `pub` field of a trait/struct.** Lock types leak the framework's concurrency model across module boundaries; every caller has to reason about lock ordering / poisoning / re-entrancy. SP-6's verdict is that locks should live behind private fields; public APIs should expose immutable snapshots or scoped callbacks.

**Patterns flagged:**
* `pub fn foo() -> RwLockReadGuard<...>` / `RwLockWriteGuard<...>` / `MutexGuard<...>` / `RwLock<...>` / `Mutex<...>`
* `pub field: (Arc<)?(parking_lot::)?(RwLock|Mutex)<...>`

**Allowlist marker:** `// PORT-CHECK-OK-SP6: <reason + tracking-issue>`. Same ±2-line window logic as trigger 10 — the marker is accepted on the preceding line, the same line, or 2 lines after the declaration (rustfmt may move trailing same-line markers on block-opening signatures like `pub fn foo() -> RwLockReadGuard {` into the body). Pre-existing leaks in the binding / context / callback-storage layers are marked individually.

**Back-references:** [architecture-correction-plan §SP-6](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U44](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### 13. Constructor-time panics

**SP-8 — `unwrap()` / `expect(` / `panic!(` / `assert!(` inside a public CONSTRUCTOR body (`pub fn new` / `pub fn from_*` / `pub fn try_*`).** Turns argument-validation bugs into process aborts at the public API surface. The SP-8 verdict is that public constructors should return `Result` or take pre-validated types.

**Allowed:** `debug_assert!` (compiled out in release).

**Mechanical scope (deliberately narrow — high precision, accepts false-negatives):** single-line constructor bodies of the shape `pub fn new(...) -> Self { ... .unwrap()/.expect()/panic!/assert! ... }` (inline body with one of the panic forms on the SAME line as the `pub fn (new|from_*|try_*)` signature). Multi-line constructor bodies are NOT inspected; rustc + clippy lints (`clippy::expect_used`, `clippy::unwrap_used`) cover that surface where opted in.

**Allowlist marker:** `// PORT-CHECK-OK-SP8: <reason>` on the same line as the panic.

**Back-references:** [architecture-correction-plan §SP-8](research/2026-05-22-architecture-correction-plan.md), [D-block plan §U45](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md).

### 14. Unit-barrier escape hatches in `flui-geometry`

**U12 — implicit scalar conversions and cross-type `f32` operators on the logical-pixel unit wrappers.** The `flui-geometry` polish pass (N-geom §U1/U2/U4/U6) removed the operations that let an untyped scalar silently cross the unit boundary. This trigger keeps them gone: the next contributor who adds "just one quick conversion" re-opens exactly the coordinate-mixing bug class the pass closed (the class Dart's `double`-everywhere policy cannot type out).

**Forbidden in `crates/flui-geometry/src/`:**

- `impl From<f32>` / `impl From<f64>` for a unit wrapper — use `px(..)` / `::new(..)` / `FloatUnit::from_f32(..)` (the named, explicit bridge for generic float-domain math) instead.
- `impl PartialEq<f32>` / `impl PartialOrd<f32>` / `impl Add<f32>` / `impl Sub<f32>` for a unit wrapper — compare/add against `px(..)`, or drop to a scalar with `.get()`.
- `pub type FloatPoint` / `FloatVec2` / `FloatSize` / `FloatOffset` — dead "GPU-ready" aliases (GPU wants `[f32; 2]` via `.to_array()`, not a `Point<T>`).

**Allowed:** `Mul<f32>` / `Div<f32>` (scaling a length by a dimensionless factor is well-defined); `From<unit> for f32`/`f64` (lossless extraction); the `FloatUnit` trait's explicit `from_f32`.

**Allowlist marker:** `// PORT-CHECK-OK-UNIT: <reason>` within ±2 lines of the declaration (e.g. `PixelDelta`'s `From<f64>`, which carries a platform scroll delta rather than a coordinate).

**Back-references:** [N-geom polish-pass research §III U1–U12](research/2026-05-24-flui-geometry-polish-pass-research.md), [ROADMAP-TRACKER N-geom block](ROADMAP-TRACKER.md).

### 15. `println!` / `eprintln!` / `dbg!` in foundation / tree / macros source

**F26 — stdout/stderr macros in the low-level substrate.** Foundation, tree, and macros are the framework's low-level substrate; they must route diagnostics through `tracing::{error,warn,info,debug,trace}!`, never stdout/stderr macros. A stray `println!` / `eprintln!` / `dbg!` in this layer leaks unstructured output into every downstream binary and is invisible to the tracing subscriber.

**Scope:** `crates/flui-foundation/src`, `crates/flui-tree/src`, `crates/flui-macros/src`.

**Exclusions:** doc-comment lines (`//!`, `///`, `//`) — example code in docs is fine; dedicated test files (`tests/`, `test*.rs`). In-file `#[cfg(test)]` modules are NOT post-filtered (unlike trigger 8): the three crates in scope keep their test output via `assert!` / `tracing`, so the path-glob exclusion of dedicated test files suffices. If a future `#[cfg(test)]` block legitimately needs `println!`, add a `test*.rs`-style split or a per-line allowlist marker in the same PR.

**Allowlist:** none today (see the `#[cfg(test)]` note above).

**Back-references:** [core-0a foundation adversarial reaudit §F26 / SC10](../openspec/changes/core-0a-foundation-adversarial-reaudit/proposal.md); [`AGENTS.md`](../AGENTS.md) "no `println!` / `eprintln!` / `dbg!` in foundation/tree/macros crates."

### 16. Module-level `#![allow(unsafe_code)]` in foundation / tree source

**F9 — blanket unsafe-allow that never self-cleans.** Edition-2024 idiom: a module that genuinely needs `unsafe` must use `#![expect(unsafe_code, reason = "...")]` so the lint fires the day the last `unsafe` block is removed; a module with no `unsafe` carries neither attribute. A blanket `#![allow(unsafe_code)]` silently permits any future unsafe and never self-cleans — it is forbidden in these two crates.

**Scope:** `crates/flui-foundation/src`, `crates/flui-tree/src`.

**Allowed:** `#![expect(unsafe_code, reason = "...")]` is the sanctioned form and does NOT match this pattern (the regex is anchored to `#![allow(unsafe_code`).

**Allowlist:** none — convert to `#![expect(...)]` or delete the attribute.

**Back-references:** [core-0a foundation adversarial reaudit §F9 / SC9](../openspec/changes/core-0a-foundation-adversarial-reaudit/proposal.md).

### 17. Reinvented `debug_assert_*` macros in foundation source

**F29 — custom assert macros that reinvent `debug_assert!`.** F29 deleted `debug_assert_valid!` / `debug_assert_range!` / `debug_assert_finite!` / `debug_assert_not_nan!` — they reinvented stdlib `debug_assert!` with no added value. This trigger prevents their reintroduction: any `macro_rules!` defining one of these four names in foundation source is a regression.

**Scope:** `crates/flui-foundation/src`.

**Allowed:** stdlib `debug_assert!` is the canonical form.

**Allowlist:** none.

**Back-references:** [core-0a foundation adversarial reaudit §F29 / SC11](../openspec/changes/core-0a-foundation-adversarial-reaudit/proposal.md).

### 18. `new_unchecked` in `flui-foundation/src/key.rs`

**F2 (P0 UB) — key counter off the checked path.** F2 replaced `NonZeroU64::new_unchecked` in `Key::new` with the `fetch_update` sentinel pattern (counter = 0 is the permanent-exhaustion sentinel; retries panic without mutation or duplicate keys), eliminating the UB-on-counter-wrap hazard. This trigger guards against reintroducing any `new_unchecked` call into `crates/flui-foundation/src/key.rs` — the key counter must stay on the safe checked path.

**Scope:** `crates/flui-foundation/src/key.rs` only.

**Exclusions:** doc-comment lines (`//!`, `///`, `//`).

**Allowlist:** none. `*_unchecked` constructors elsewhere (e.g. `id.rs`) are out of scope and governed by their own `#![expect(unsafe_code, ...)]`.

**Back-references:** [core-0a foundation adversarial reaudit §F2 / SC2](../openspec/changes/core-0a-foundation-adversarial-reaudit/proposal.md) (Rustonomicon §3.2 is the UB basis).

### 19. `Matrix4` in the DrawBatcher record side, `PipelineCache`/`PipelineBuilder`, or `GpuReplay` replay side

**C4 rule: the `Matrix4`↔glam conversion happens at the `Backend` trait boundary; the record, pipeline, and replay modules are glam-only.** `GpuStateStack` stores transforms as `glam::Mat4`. The single structural conversion edge is `current_transform_matrix()` in `painter.rs` (outbound, returning a `Matrix4` to `Backend`/`LayerStateStack`) and `Backend::with_transform` (inbound, converting an incoming `Matrix4` into the glam state). Every record method below that boundary — `batches/{shapes,gradients,paths,images}.rs` — the pipeline-cache module (`pipelines.rs`), and the replay/submit module (`replay.rs`) work entirely in glam primitives and pixel-typed geometry.

Importing or accepting `flui_types::Matrix4` on the record/pipeline/replay side leaks the flui-types coordinate abstraction into GPU plumbing, defeats the `GpuStateStack` encapsulation, and couples every record-method caller to both coordinate systems. The replay side must stay glam-only for the same reason: the `Matrix4`↔glam conversion must not migrate into the GPU-emit path. The correct fix is always to extract the needed scalars (translation, scale) at the `painter.rs` or `backend.rs` call site and pass primitives down.

**Scope:** `crates/flui-engine/src/wgpu/batches/` (all files), `crates/flui-engine/src/wgpu/pipelines.rs`, and `crates/flui-engine/src/wgpu/replay.rs`. (Extended to `replay.rs` in T10e — the scope tracks the seam contract: wherever the record-IR is consumed, the glam-only rule applies.)

**Allowlist:** none. Doc-comment lines (`//!`, `///`, `//`) are excluded (the rg filter strips them).

**Back-references:** [`docs/adr/ADR-0006-c-ir-record-replay-seam.md`](adr/ADR-0006-c-ir-record-replay-seam.md) §Decision 4 (C4 rule); engine-overhaul spec `.rust-studio/specs/flui-engine-overhaul/spec.md` acceptance criterion C4; `crates/flui-engine/ARCHITECTURE.md` §Record/replay boundary.

### N-geom.U16. Direct `glam` use outside the wgpu backend

**Option D's glam policy is an engine-edge policy.** `glam` is sanctioned for GPU/painter hot-path math under `crates/flui-engine/src/wgpu/`, where typed `flui_geometry` values are converted into SIMD/Pod-friendly GPU primitives. Direct `glam::...` or `use glam...` code outside that backend widens the bridge policy into unrelated engine modules and bypasses the FLUI-owned public geometry surface documented in `crates/flui-types/README.md`.

**Scope:** `crates/flui-engine/src`, excluding `crates/flui-engine/src/wgpu/**`.

**Allowlist:** none. Add a documented bridge or move the conversion to the wgpu edge instead of importing `glam` directly in other engine modules.

**Back-references:** `docs/ROADMAP-TRACKER.md` `N-geom.U16`; `crates/flui-engine/src/wgpu/mod.rs` §Math-backend policy; `crates/flui-types/README.md` FAQ "Why not use glam or euclid?".

### Cross.H3. `ElementBuildContext::new_minimal` resurrection

**Production builds must receive a live tree-backed `BuildContext`.** Catalog theming is an `InheritedView` consumer, so `build()` must resolve inherited providers, ancestor walks, render-object lookup, and notification bubbling against the real `ElementTree`. The old `ElementBuildContext::new_minimal` dummy context made those APIs silently return `None`/`false` during production builds and is not a valid fallback.

**Scope:** `crates/flui-view/src`.

**Allowlist:** none. Tests and helper code that need a context should use `ElementBuildContext::for_element` over a real tree or drive `BuildOwner::build_scope`, which supplies the borrowed live `BuildCtx`.

**Back-references:** `docs/ROADMAP-TRACKER.md` `H3`; `docs/designs/2026-06-25-pr-k-live-buildcontext-execution-spec.md`; `crates/flui-view/src/context/element_build_context.rs` `BuildCtx`.

### 20. Gradient/image SrcOver warn-fallback strings in producer files

**PR-5 deleted three warn-fallback blocks** that previously made gradient and image producers silently fall through to SrcOver when an advanced (dst-read) blend mode was requested. If any of the deleted strings reappear in `batches/`, `renderer.rs`, or `backend.rs`, a producer has regressed to the fallback path: callers requesting Multiply, Screen, Overlay, etc. will silently receive SrcOver output instead of the correct advanced blend result.

The two sentinel patterns are `"is not supported by the"` and `"rendering as SrcOver"`. Both were exclusive to the deleted warn-fallback blocks; their reappearance on the producer side is unambiguous evidence of regression.

**Scope:** `crates/flui-engine/src/wgpu/batches/` (all files), `crates/flui-engine/src/wgpu/renderer.rs`, `crates/flui-engine/src/wgpu/backend.rs`. `replay.rs` is explicitly excluded — it is the replay/submit side, not a producer, and may legitimately use similar language in its own documentation.

**Runtime companion:** `PipelineCache::get_or_create` contains a `debug_assert!(!key.blend_mode().is_advanced(), …)` that panics in debug/test builds if any advanced mode reaches the pipeline cache instead of diverting to `DrawItem::AdvancedShape`. This is the runtime half of the gate; the static grep above is the compile-time half. Both must remain active.

**Allowlist:** none. A producer genuinely unable to support advanced blend must divert to `DrawItem::AdvancedShape` (reusing `render_segment_to_offscreen` + `flush_advanced_layer`) — not warn and fall through.

**Witnesses — two tiers:**

- **Routing witnesses (CI-runnable, no pixel readback):** CPU unit tests G1-G3 in `crates/flui-engine/src/wgpu/batches/mod.rs` (gradient path); GPU-device structure tests I1-I5 in `crates/flui-engine/src/wgpu/gradient_image_blend_tests.rs` (image/atlas paths — each asserts exactly one `DrawItem::AdvancedShape` per call with the correct `cached_images.len()`). These are the authoritative routing witnesses for condition 3.

- **Non-panic + non-zero-output witness:** GPU test GI7 (`crates/flui-engine/src/wgpu/gradient_image_blend_tests.rs`) verifies all 15 modes × gradient + image produce valid RGBA output. GI7 does not verify routing (pixel equality alone cannot distinguish an `AdvancedShape` from a lucky SrcOver result); the routing witnesses above provide that guarantee. GI8 covers the atlas producer.

**Back-references:** advanced-blend PR-5 (gradient + image + atlas diversion); `crates/flui-engine/src/wgpu/batches/gradients.rs` §dispatch_shader_rect advanced diversion; `crates/flui-engine/src/wgpu/batches/images.rs` §draw_image/draw_image_repeat/draw_image_nine_slice/draw_atlas advanced diversion; `crates/flui-engine/src/wgpu/gradient_image_blend_tests.rs` I1-I5 (routing), GI7 (non-panic + non-zero), GI8 (atlas GPU output).

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

## Dart → Rust type map

This table is the canonical lookup when translating a single Dart symbol into Rust. When a row says "see §X", read that section before choosing. When two rows could both apply, the **more specific** row wins (e.g. `BuildContext` overrides the generic `Object` rule).

### Primitive and core-library types

| Dart | Rust | Notes |
|---|---|---|
| `int` | `i64` for arithmetic that crosses Dart-int range; `i32` / `u32` / `usize` where the source range proves narrower | Dart `int` is 64-bit native; `int` indices that feed `List` are `usize` after the bounds proof. Use `i64::try_from` at the boundary if narrowing. |
| `double` | `f64` | 1:1. Never `f32` unless the source explicitly used a 32-bit type. |
| `bool` | `bool` | 1:1. |
| `String` | see [§Strings discipline](#strings-discipline) — `String` for owned mutable UI text, `&str` for borrowed, `Box<str>` for written-once message fields, `Cow<'static, str>` for "literal or owned", `Arc<str>` for shared interned, **never** `String` for syscall bytes |  |
| `List<T>` (growable) | `Vec<T>` (or `SmallVec<[T; N]>` for hot small-N — `smallvec` workspace dep) | Pre-allocated with `Vec::with_capacity(n)` mirrors Dart `List.filled` / `List.generate` capacity hints. |
| `List<T>` (fixed-length, `growable: false`) | `Box<[T]>` (or `[T; N]` if `N` is `const`) | Captures the immutable-length invariant; `Box<[T]>` skips the spare capacity word. |
| `UnmodifiableListView<T>` | `&[T]` borrow when lifetime allows; otherwise `Arc<[T]>` | `Arc<[T]>` lets the view be cloned cheaply with a shared payload. |
| `Map<K, V>` (default) | `HashMap<K, V, ahash::RandomState>` or `ahash::AHashMap<K, V>` | Default workspace hasher is `ahash` — faster than `SipHash`. Use the `std::collections::HashMap` default hasher **only** when the keys cross an FFI boundary or carry an attacker-controlled-input risk; that contradicts the default and warrants a `// PORT NOTE`. |
| `Map<K, V>` (ordered iteration required) | `BTreeMap<K, V>` | Dart `LinkedHashMap` insertion-order is approximated by `indexmap::IndexMap` for non-comparable keys — workspace does not yet depend on `indexmap`; flag with `// TODO(port)` if needed. |
| `Set<T>` | `ahash::AHashSet<T>` or `HashSet<T, ahash::RandomState>` | Same hasher rationale as `Map`. |
| `Iterable<T>` (lazy) | `impl Iterator<Item = T>` (or `&dyn Iterator<Item = T>` at FFI boundary — needs `// PORT-CHECK-OK-DYN` marker per Trigger 9) | Strict-eager → `Vec<T>`. |
| `Future<T>` | `impl Future<Output = T>` (or `Pin<Box<dyn Future<Output = T> + Send>>` at FFI/storage — exempted by FR-029) | **Forbidden** on hot path (Refusal trigger 3). Permitted in `flui-assets`, `flui-scheduler`, `flui-build`. Use `tokio::task::spawn` only at those boundaries. |
| `Stream<T>` | `impl futures::Stream<Item = T>` or `tokio::sync::broadcast::Receiver<T>` | **Forbidden** on hot path. UI change-notification → `Listenable` trait + manual notify loop, not `Stream`. |
| `dynamic` | **forbidden** — convert to a typed surface. If literally unavoidable at an FFI boundary: `&dyn Any` with `// PORT-CHECK-OK-DYN: <reason>` and a `downcast_ref::<ConcreteT>` site marked with `// PORT-CHECK-OK-DOWNCAST: <reason>` | Constitution Principle IV forbids open-world `dyn`. The Dart `dynamic` keyword is a port-time conversation, not a 1:1 mapping. |
| `Object` (untyped base) | concrete type, or `&dyn Any` with markers (same rule as `dynamic`) | Most Flutter uses of `Object` are debugging payloads or untyped equality keys; pick the concrete type from the call graph. |
| `T?` (nullable) | `Option<T>` | `null` literal → `None`. |
| `Object?` (nullable untyped) | `Option<Box<dyn Any + Send>>` at FFI boundary only — usually a sign the source needs a typed enum | Flag with `// TODO(port): typed-enum candidate`. |
| `void` (return) | `()` | Never `Result<(), ()>` — use `Result<(), ErrorType>` if fallible. |
| `Never` (return) | `!` (never type) or `core::convert::Infallible` | Diverging fns use `-> !`; type-system slot for "this Result branch is impossible" uses `Infallible`. |
| `Function` (untyped) | **forbidden** — narrow to `fn(Args) -> R` (zero-overhead fn pointer) or `Box<dyn Fn(Args) -> R + Send + Sync>` (owned callback storage, sanctioned by FR-029 #5) | Typed function pointers always win when the call site has a fixed signature. |
| `typedef Cb = void Function(int)` | `type Cb = fn(i32);` for zero-overhead; `type Cb = Box<dyn Fn(i32) + Send + Sync>;` for owned storage | Owned-storage variant carries the `+ Send + Sync` bound to interop with `Listenable` plumbing. |
| `Symbol` | `&'static str` or `core::any::TypeId` | Use `TypeId` for the `InheritedView` registry; use `&'static str` for `debug_name` slots. |
| `DateTime` | `std::time::SystemTime` (wall clock) or `std::time::Instant` (monotonic) | Use `Instant` for frame timing; `SystemTime` for serialised timestamps only. |
| `Duration` | `std::time::Duration` | 1:1. |
| `Uri` | `url::Url` (requires adding `url` to `[workspace.dependencies]` — currently only a transitive dep of `reqwest`, not declared at workspace root) | Path-only URIs may use `&Path` / `PathBuf` without the extra dep. |

### Nullability and late initialization

| Dart | Rust | Notes |
|---|---|---|
| `late T x;` (single-init, throws if read before set) | `OnceCell<T>` (`once_cell::unsync::OnceCell` if `!Send`, `once_cell::sync::OnceCell` otherwise) | Read returns `Option<&T>`. Initial `panic` on uninit-read mirrors Dart's `LateInitializationError`. |
| `late final T x;` | `OnceCell<T>` (set-once) or `LazyLock<T>` (init by fn) | Stable since Rust 1.80 (`std::sync::LazyLock` replaces `once_cell::sync::Lazy`). |
| `late T x = compute();` (eager-on-first-read) | `LazyLock<T, fn() -> T>` | Stable in std since 1.80; prefer over `once_cell::sync::Lazy`. |
| `static late final T = ...` (process-wide singleton) | `static FOO: LazyLock<T> = LazyLock::new(\|\| ...)` | If the init can fail, use `OnceLock<T>` + explicit init at boot. |
| `T? x;` (optional field, set lazily) | `Option<T>` (preferred) — or interior mutability when the parent is borrowed `&self`: `Cell<Option<T>>` if `T: Copy`, `RefCell<Option<T>>` otherwise, `OnceCell<T>` for set-once semantics | `Option<T>` is the default. `Cell` requires `Copy` (it returns by value on `.get()`); reach for `RefCell` when `T` is non-`Copy`, and `OnceCell` when the lazy-init contract is "set exactly once". |

### Flutter framework types

| Flutter | flui | Crate / notes |
|---|---|---|
| `Widget` | `impl View` (return) / `&dyn View` (param, FR-029 sanctioned, allowed in port-check) / `BoxedView` (heterogeneous storage) | `flui-view`. Storage form is `Element<V, A, B>` — see §Refusal trigger 6. |
| `StatelessWidget` | `View` impl whose `build(&self, &BuildContext)` returns `impl View` | `flui-view`. |
| `StatefulWidget` + `State<T>` | typestate-style `View` + `Element` pair — `StatefulElement<S>` owns the state | `flui-view`. State is an arena field, not a separate object. |
| `InheritedWidget` | `InheritedView` + `TypeId` registry | `flui-view`. `TypeId::of::<T>()` lookup is the single allowed runtime-reflection window per strategy. |
| `BuildContext` | `&BuildContext<'_>` borrowed; opaque accessor surface | `flui-view`. **Always borrowed**, never owned, never stored across `await` (moot — sync hot path). |
| `Element` | arena-allocated; reached via `ElementId` (`NonZeroUsize` newtype) | `flui-view`. `Slab<ElementCore>` storage; tree links via IDs, not pointers. |
| `RenderObject` | `Box<dyn RenderObject<P>>` (plain field — sanctioned by FR-029) inside `RenderEntry<P>` | `flui-rendering`. `P` is the protocol (Box / Sliver); the trait is open-set. |
| `Layer`, `ContainerLayer`, `PictureLayer`, `OffsetLayer`, `OpacityLayer`, etc. | closed `Layer` enum + `Vec<LayerId>` children on container variants | `flui-layer`. No `Box<dyn Layer>` — see [`crates/flui-layer/ARCHITECTURE.md`](../crates/flui-layer/ARCHITECTURE.md) "closed Layer enum" decision. |
| `Canvas` | `&mut Canvas` borrow; backing `DisplayList` accumulates `DrawCommand` enum variants | `flui-painting`. |
| `Paint` | `Paint` value-type, `Copy` where possible | `flui-painting`. |
| `Picture` / `Scene` | `DisplayList` (recorded) → `LayerTree` (composited) → wgpu draw | `flui-painting` → `flui-layer` → `flui-engine`. |
| `Rect`, `RRect`, `Offset`, `Size`, `EdgeInsets` | identically-named structs in `flui-types` | `flui-types`. `Copy` types. |
| `Color` | `Color` (`flui-types`, sRGB by default) | `flui-types`. Alpha is straight (not pre-multiplied) at the API surface; the engine pre-multiplies before upload. |
| `Key` (`ValueKey`, `ObjectKey`, `UniqueKey`, etc.) | `ViewKey` trait (sanctioned by FR-029) | `flui-foundation` owns the trait and base key types; `flui-view` owns `ObjectKey` / `GlobalKey`. Storage on `ElementNode` is `Option<Box<dyn ViewKey>>`, populated via `ViewKey::clone_key()`. |
| `GlobalKey<T>` | typestate-checked `GlobalKey<T>` — separate machinery, not all keys are global | `flui-view`. |
| `Notification` | `Notification` trait (sanctioned by FR-029) + `NotifiableElement` | `flui-view`. Bubble dispatch via element walk. |
| `ChangeNotifier` | `Listenable` trait (sanctioned by FR-029) | `flui-foundation`. Multiple impls; `ChangeNotifier` struct is a default fan-out impl. |
| `ValueNotifier<T>` | `ValueNotifier<T>` struct implementing `Listenable` | `flui-foundation`. |
| `ValueChanged<T>` callback | `Arc<dyn Fn(T) + Send + Sync>` (owned storage — matches `crates/flui-foundation/src/callbacks.rs:70`) or `&dyn Fn(T)` (borrowed param) | Storage form sanctioned by FR-029 #5. `Arc` not `Box` because the listener registry clones callbacks across notifier fan-out. Note: `crates/flui-foundation/ARCHITECTURE.md:62` is stale and still says `Box<dyn Fn(T)>` — graft pending. |
| `VoidCallback` | `Arc<dyn Fn() + Send + Sync>` (storage — matches `crates/flui-foundation/src/callbacks.rs:51`) or `&dyn Fn()` (param) | Same. |
| `AnimationController`, `Animation<T>`, `CurvedAnimation` | `Animation<T>` trait (sanctioned by FR-029) + concrete impls | `flui-animation` (active; see `## Index`). |
| `Listenable` (Dart base class) | `Listenable` trait — `flui-foundation` | Multiple-source: also see `Animation` for animation-as-listenable. |
| `mixin Foo on Bar` | `trait Foo` + `#[delegate(Foo)]` via `ambassador` (workspace dep) | See [§Dart → Rust idiom map](#dart--rust-idiom-map) row "mixin". |

### Sentinel and error types

| Dart | Rust | Notes |
|---|---|---|
| `throw FlutterError(...)` | `return Err(<CrateError>::Variant { ... })` | See [§Error mapping canonical shape](#error-mapping-canonical-shape). |
| `assert(x)` / `assert(x, "msg")` | `debug_assert!(x)` / `debug_assert!(x, "msg")` | Stripped in release. |
| `unreachable` (Dart marker for `// ignore: dead_code`) | `unreachable!()` (panic on hit) or `unreachable_unchecked()` only in `unsafe` proven-impossible spots | Default = `unreachable!()`. The `_unchecked` form requires a proof comment + a `// SAFETY:` marker. |
| `runtimeType` | `core::any::TypeId::of::<T>()` (registry lookup) or `core::any::type_name::<T>()` (debug name) | Type name is monomorphized; calling through `&dyn Trait` returns the concrete type via vtable. |
| `is Foo` | `matches!(x, Foo { .. })` for an enum variant; `<Any as Any>::is::<Foo>()` for typed `Any`-cast | The `Any::is` form requires `&dyn Any` first. |
| `as Foo` (typed downcast) | `x.downcast_ref::<Foo>()` / `x.downcast_mut::<Foo>()` with `// PORT-CHECK-OK-DOWNCAST: <reason>` marker | Bare cast (`x as Foo`) does not apply — Dart `as` is a typed downcast, not a numeric cast. |
| `Iterable<T>.cast<U>()` | typed channel, not `.cast` — convert to `Vec<U>` via `.into_iter().map(Into::into).collect()` | Cast is a Dart wart; the Rust shape forces an explicit conversion. |

---

## Dart → Rust idiom map

Patterns, not types. When a Dart construct could compile to multiple Rust shapes, the first row is the default; subsequent rows are exceptions.

### Control flow

| Dart pattern | Rust pattern | Notes |
|---|---|---|
| `if (cond) { ... } else { ... }` | `if cond { ... } else { ... }` | 1:1. Rust `if` is an expression. |
| `cond ? a : b` (ternary) | `if cond { a } else { b }` | No ternary; the `if` expression covers it. |
| `switch (x) { case A: ...; case B: ...; default: ... }` | `match x { A => ..., B => ..., _ => ... }` | `match` is exhaustive by default. Drop the `default` arm if you've named every variant. |
| `switch (x) { case A when cond: ... }` (pattern + guard, Dart 3) | `match x { A if cond => ..., ... }` — or `match x { A if let Some(v) = inner => ... }` (Rust **1.95+** if-let guards) | The if-let-guard form is stable since 1.95. |
| `for (var x in iter) { ... }` | `for x in iter { ... }` | If `iter` is owned and consumed, `.into_iter()` is implicit. |
| `for (var i = 0; i < n; i++) { ... }` | `for i in 0..n { ... }` | Half-open range. |
| `while (cond) { ... }` | `while cond { ... }` | 1:1. |
| `do { ... } while (cond);` | `loop { ...; if !cond { break; } }` | Rust has no `do-while`. |
| `try { ... } catch (e) { ... }` | `match fallible() { Ok(v) => ..., Err(e) => ... }` or `let v = fallible()?;` for propagation | Rust has no exceptions; everything goes through `Result`. |
| `try { ... } on FlutterError catch (e) { ... }` (typed catch) | `match fallible() { Err(RenderError::Specific(...)) => ..., Err(e) => return Err(e), Ok(v) => ... }` | Per-variant matching. |
| `try { ... } finally { cleanup(); }` | `let _guard = scopeguard::guard((), \|_\| cleanup());` (`scopeguard` is not currently a workspace dep — flag with `// TODO(port): add scopeguard` and inline a `Drop` wrapper if needed) | For the common case where cleanup is `drop` on owned values, scope exit is automatic and no guard is needed. |
| `throw error;` | `return Err(error);` | See [§Error mapping](#error-mapping-canonical-shape). |
| `rethrow;` | `return Err(e);` after capturing in a previous arm | Rust has no `rethrow` keyword; explicit re-return. |

### Optional / null

| Dart | Rust | Notes |
|---|---|---|
| `x ?? y` | `x.unwrap_or(y)` (eager) or `x.unwrap_or_else(\|\| y)` (lazy) | Eager is fine for cheap defaults; lazy for non-trivial. |
| `x ??= y` | `x.get_or_insert(y)` where `x: &mut Option<T>` | Returns `&mut T` to the now-Some inner. Use `get_or_insert_with(\|\| y)` if `y` is non-trivial. |
| `x?.method()` | `x.as_ref().map(\|v\| v.method())` or `if let Some(v) = x.as_ref() { v.method() }` | `.as_ref()` borrows the Option's content. |
| `x?.method() ?? default` | `x.as_ref().map(\|v\| v.method()).unwrap_or(default)` | Chained. |
| `x!` (null-assertion) | `x.unwrap()` or `x.expect("invariant: …")` | Prefer `.expect` with a context string in framework code. |

### Closures and callbacks

| Dart | Rust | Notes |
|---|---|---|
| `(int x) => x * 2` (arrow lambda) | `\|x: i32\| x * 2` | Type annotation usually elided. |
| `(int x) { return x * 2; }` (block lambda) | `\|x: i32\| { x * 2 }` | Same. |
| `void Function() cb = () { ... };` (storage) | `let cb: Box<dyn Fn() + Send + Sync> = Box::new(\|\| { ... });` | Storage form per FR-029 #5. |
| `cb()` (invocation) | `cb()` | Boxed closures call directly. |
| capture by reference (Dart default) | move closures explicitly with `move \|\| { ... }` when crossing threads | Rust closure capture is inferred; `move` forces by-value. |

### Object model

| Dart | Rust | Notes |
|---|---|---|
| `class Foo { final int x; Foo(this.x); }` | `pub struct Foo { pub x: i32 } impl Foo { pub fn new(x: i32) -> Self { Self { x } } }` | Constructor naming convention: `new` for primary, `with_*`/`from_*` for alternates. |
| `factory Foo.fromX(int n) => Foo._internal(n * 2);` | `impl Foo { pub fn from_x(n: i32) -> Self { Self::_internal(n * 2) } }` | Factory just maps to an associated `fn`. |
| `const Foo(this.x);` (const constructor) | `impl Foo { pub const fn new(x: i32) -> Self { Self { x } } }` | Use `const fn` only when the body is const-eligible. |
| `class Foo extends Bar` (inheritance) | `impl Bar for Foo` (trait impl) — no inheritance | Add an `inner: Bar` field if behavior reuse via composition is needed. |
| `class Foo extends Bar with M1, M2` (mixin) | `impl Foo { ... } #[derive(Delegate)] #[delegate(M1, target = "inner")] #[delegate(M2, target = "inner")]` | `ambassador` is the workspace dep for delegation. |
| `super.method()` (call parent impl) | call the delegated field directly: `self.inner.method()` | No automatic super-dispatch. |
| `@override void foo() { ... }` | `impl Trait for Foo { fn foo(&mut self) { ... } }` | Trait method placement. |
| `@protected` / `@visibleForTesting` | `pub(crate)` for protected; `#[cfg(test)]` for test-only | No annotation equivalent. |
| `@deprecated` | `#[deprecated(note = "...", since = "...")]` | 1:1. |
| `..` cascade (`obj..a()..b()`) | builder method chain returning `&mut Self` or `Self`: `obj.a().b()` | For typestate builders use `bon` (workspace dep). |
| `==` operator override | `#[derive(PartialEq, Eq)]` (structural) or hand-written `impl PartialEq` (custom) | Always pair `Eq` with `Hash` — Rust does not enforce it; we do. |
| `hashCode` getter override | `#[derive(Hash)]` (structural) or `impl Hash` (custom — must agree with custom `PartialEq`) | Same. |
| `toString()` override | `impl Display for Foo` (or `impl Debug`) | `Display` is the human form; `Debug` is the diagnostic form. |
| operator overload (`Offset operator +(Offset other)`) | `impl Add for Offset { type Output = Self; fn add(self, rhs: Self) -> Self { ... } }` | `core::ops::*` traits. |
| `class Foo<T extends Bar>` (generic with bound) | `pub struct Foo<T: Bar> { ... }` or `pub struct Foo<T> where T: Bar { ... }` | `where` clause for >1 bound. |

### Compile-time-conditional code

| Dart | Rust | Notes |
|---|---|---|
| `if (kIsWeb) { ... }` / `Platform.isWindows` | `#[cfg(target_arch = "wasm32")]` / `#[cfg(windows)]` — or `cfg_select! { target_os = "windows" => ..., _ => ... }` (Rust **1.95+**) | `cfg_select!` was stabilized in 1.95 — prefer it over paired `#[cfg(...)]` + `#[cfg(not(...))]` blocks. |
| `if (kReleaseMode) { ... }` | `#[cfg(not(debug_assertions))]` | 1:1. |
| `assert(x)` (stripped in release) | `debug_assert!(x)` | 1:1. |
| `const x = ...` (compile-time constant) | `const X: T = ...;` | Item-level; types must be `const`-eligible. |

### Iteration

| Dart | Rust | Notes |
|---|---|---|
| `iter.map((x) => f(x))` | `iter.map(\|x\| f(x))` or `iter.map(f)` | Drop the closure if `f` matches. |
| `iter.where((x) => p(x))` | `iter.filter(\|x\| p(x))` | Rust uses `filter`. |
| `iter.fold(init, (acc, x) => g(acc, x))` | `iter.fold(init, \|acc, x\| g(acc, x))` | 1:1. |
| `iter.reduce((a, b) => g(a, b))` | `iter.reduce(\|a, b\| g(a, b))` — returns `Option<T>` (empty iter = None) | Behavior matches Dart's empty-iter throw if `.unwrap()` follows; prefer explicit handling. |
| `iter.toList()` | `iter.collect::<Vec<_>>()` | Or `.collect()` if the target type is inferred. |
| `iter.toSet()` | `iter.collect::<ahash::AHashSet<_>>()` | Match the hasher rule in [§type map](#dart--rust-type-map). |
| `iter.length` (eager) | `iter.count()` (consumes) or `iter.size_hint()` (peek-only) | `.count()` walks the iterator; cache it if reused. |
| `list.length` (O(1)) | `vec.len()` | 1:1. |
| `list[i] = x` | `vec[i] = x;` | Panics on out-of-bounds in both. |
| `list.add(x)` | `vec.push(x);` (returns `()` in stable; **Rust 1.95+** `vec.push_mut(x)` returns `&mut T` to the inserted slot) | Use `push_mut` when the next op mutates the new element in-place. |
| `list.insert(i, x)` | `vec.insert(i, x);` (`()` return; Rust 1.95+ `vec.insert_mut(i, x)` returns `&mut T`) | Same. |
| `list.removeLast()` | `vec.pop()` returns `Option<T>` | Dart returns the element; Rust returns Option. |
| `list.removeAt(i)` | `vec.remove(i)` (O(n)) or `vec.swap_remove(i)` (O(1) if order doesn't matter) | Prefer swap_remove for hot paths. |
| `list.clear()` | `vec.clear()` (keeps capacity) | 1:1. |
| `map[k] = v` | `map.insert(k, v);` | Returns `Option<V>` (previous value). |
| `map[k]` (lookup) | `map.get(&k)` returns `Option<&V>` | Indexing operator panics on missing in std HashMap. |
| `map.containsKey(k)` | `map.contains_key(&k)` | 1:1. |
| `map.putIfAbsent(k, () => v())` | `map.entry(k).or_insert_with(\|\| v())` | Entry API avoids double lookup. |

### Concurrency primitives

| Dart | Rust | Notes |
|---|---|---|
| `Future.delayed(Duration)` | `tokio::time::sleep(Duration)` (async edges only — Refusal trigger 3) | Forbidden on hot path. |
| `Future.wait([a, b])` | `tokio::join!(a, b)` or `futures::future::join_all(...)` | Edges only. |
| `Stream.broadcast()` | `tokio::sync::broadcast::channel(cap)` | Edges only. UI change-notification → `Listenable`, not `Stream`. |
| `Completer<T>` | `tokio::sync::oneshot::channel::<T>()` | Edges only. |
| `Isolate.spawn(...)` (separate heap) | `std::thread::spawn(...)` or `tokio::task::spawn_blocking(...)` | flui has no isolate model; threads share memory via `Arc`. |
| `compute(...)` (Dart background-isolate worker) | `tokio::task::spawn_blocking(...)` (CPU-bound) or `rayon::spawn(...)` (parallel iter — `rayon` not currently in workspace; flag with `// TODO(port)`) | flag with `// TODO(port): add rayon` if parallel reduction is needed. |
| `synchronized(...)` (Dart `synchronized` package) | `parking_lot::Mutex<T>::lock()` returns a guard | `parking_lot` is the workspace default (smaller, faster, no poisoning). |

### Atomics

Rust **1.95+** added `update()` / `try_update()` on `AtomicBool`, `AtomicIsize`, `AtomicUsize`, `AtomicPtr` — they encapsulate the standard `load → fn → compare_exchange_weak` loop into a single call. The dirty-flag CAS sites in `crates/flui-rendering/src/storage/state.rs` are candidates for the cleanup (track with `// PERF(port): pre-1.95 CAS loop, swap to update()` so the Phase B pass finds them).

### Strings (cross-reference)

Strings are covered fully in [§Strings discipline](#strings-discipline). The short form:

| Dart | Rust | Notes |
|---|---|---|
| `'hello ' + name` (concat) | `format!("hello {name}")` (heap-allocs) | **Not** on hot path. |
| `'$x + $y = ${x+y}'` (interpolation) | `format!("{x} + {y} = {sum}", sum = x + y)` | 1:1 capture syntax (Rust 1.58+). |
| `'abc'.length` (UTF-16 code units in Dart) | `s.chars().count()` (Unicode scalars) — **not** `s.len()` (UTF-8 bytes) | Semantics differ; the right answer depends on what the Dart code meant. Flag with `// PORT NOTE: char count vs byte len`. |
| `'abc'.toLowerCase()` | `s.to_lowercase()` (Unicode-aware) | Heap-allocates. |
| string equality | `a == b` | 1:1 for `&str` and `String`. |

### Mixin → trait + ambassador (worked example)

Dart:
```dart
mixin DiagnosticableMixin {
  String toStringShort() => runtimeType.toString();
  Map<String, Object?> toDiagnostics() => {};
}

class Foo with DiagnosticableMixin {
  // inherits both methods
}
```

Rust:
```rust
pub trait Diagnosticable {
    fn to_string_short(&self) -> String;
    fn to_diagnostics(&self) -> ahash::AHashMap<String, Box<dyn core::any::Any>>;
}

// Default impl using ambassador for delegate-style reuse
#[derive(ambassador::Delegate)]
#[delegate(Diagnosticable, target = "inner")]
pub struct Foo {
    inner: DefaultDiagnostics,
}
```

The `inner` field carries the default behavior; `ambassador` generates the delegating impl. When a method needs an override, write the trait impl by hand and skip the delegate for that one method.

---

## Strings discipline

Dart conflates "UI text", "identifier", "syscall byte sequence", "source code", and "encoded resource" into a single `String` type (UTF-16 internal, UTF-8 on the wire). Rust forces a choice. The rules below resolve the choice deterministically by **what the data is**, not where it came from.

### Decision tree

1. **UI text the user reads** (`Text("Hello")` content, `TextSpan.text`, button labels) → `String` (owned mutable, growable) or `&str` (borrowed). Always valid UTF-8 by Rust invariant.

2. **Written-once message fields** on error variants, debug-name slots, log payloads → `Box<str>`. One header word smaller than `String`, no growth amortization needed. This is the **established flui convention** (precedent: `crates/flui-rendering/src/error.rs` R-17, all `RenderError` variants).

3. **Static identifiers** (trait `debug_name` returning `&'static str`, layer-kind tags) → `&'static str`. Zero allocation, comparable by pointer.

4. **Literal-or-owned strings** (rare, but useful for "default = literal, user-customizable = String") → `Cow<'static, str>`. The crate must justify why `String`+`&'static str` split is insufficient — usually not worth it.

5. **Shared interned strings** (debug labels referenced from many sites; widget keys that repeat) → `Arc<str>`. Cheaper to clone than `Arc<String>` (one fewer indirection). For true interning with deduplication, use `lasso` (already in `[workspace.dependencies]` with the `multi-threaded` feature) — its `ThreadedRodeo` returns `Spur` handles that are smaller than `Arc<str>` and the dedupe table amortises across the program. Prefer `lasso` for high-cardinality identifier interning (debug names, registered widget keys); reach for `Arc<str>` for low-cardinality one-off shared strings where the interner overhead is not worth it.

6. **Byte sequences from the outside world** (asset blobs, hot-reload source code, image bytes, network payloads) → `Vec<u8>` / `&[u8]` / `Box<[u8]>`. **Never** `String::from_utf8_lossy` on external data without an explicit `// PORT NOTE: lossy is acceptable here because …` justification — that operation silently rewrites U+FFFD over surrogate fragments and invalid sequences. If the data must round-trip, keep it as bytes.

7. **Filesystem paths** → `std::path::PathBuf` (owned) / `&std::path::Path` (borrowed). `PathBuf` handles OS-specific encoding (UTF-16 on Windows, bytes on Unix). **Never** `String` for paths — Windows paths can contain unpaired surrogates that `String` rejects.

8. **Widget keys, identifier-shaped data** (`ValueKey<&'static str>`, `ObjectKey`, debug IDs) → concrete `ViewKey` impls (`ValueKey`, `ObjectKey`, `UniqueKey`, `Key`, `GlobalKey<T>`), stored as `Box<dyn ViewKey>` only at the sanctioned element-key boundary. Do not flatten identity to `String`.

### Anti-patterns

- `String::from_utf8(bytes).unwrap()` on external data — **forbidden**. Replace with explicit error handling: `String::from_utf8(bytes).map_err(|e| MyError::InvalidUtf8 { source: e.utf8_error() })?;`.
- `s.to_string()` on a `&str` parameter you're just going to return — wasteful. Return `&str` with the right lifetime, or take `impl Into<String>` to defer the allocation to the caller.
- `String` interpolation in a hot-path log line — `format!` heap-allocates. Use `tracing::debug!(?value, "context")` which lazily formats only when the subscriber accepts the event.
- `String` for a `pub fn name(&self) -> String` that returns a constant — return `&'static str`.

### Performance footnote

`String` push-and-grow is O(1) amortized; if you know the final size, `String::with_capacity(n)` skips reallocations. For very small inline strings, `compact_str::CompactString` packs ≤24-byte strings into the stack (inline-or-heap, like `SmallVec`). It is **not** currently a workspace dep — adopting it is a port-time decision when a hot path measures string allocation cost (track with `// PERF(port): adopt compact_str if profiled hot`).

---

## Error mapping canonical shape

flui's error handling is codified across `flui-rendering`, `flui-engine`, and `flui-view`. The pattern below is the canonical shape — diverging from it requires a `// PORT NOTE` and a per-crate `## Mapping decisions` entry.

### The canonical shape

Library crates define a single error type per crate (e.g., `RenderError`, `EngineError`, `LayoutError`) using `thiserror`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RenderError {
    #[error("layout cycle detected involving {nodes} nodes")]
    LayoutCycle { nodes: usize },

    #[error("render object panicked during {phase}: {message}")]
    Poisoned {
        phase: Phase,
        message: Box<str>,
    },

    #[error("upstream IO failure")]
    Io(#[from] std::io::Error),
}

pub type RenderResult<T> = Result<T, RenderError>;
```

### Rules

1. **Use `thiserror`** for derive macros (`#[derive(Error)]` + `#[error("...")]`). Workspace dep, already adopted by every error-bearing crate.

2. **Use `#[non_exhaustive]`** on every public error enum. Lets variants be added without breaking downstream `match` exhaustively. Crates internal to a feature boundary may omit it.

3. **`Box<str>` is preferred for message fields on new error types**, not `String`. Errors are written-once / read-rarely; the spare-capacity word of `String` is wasted. **Codebase state:** `flui-rendering` follows this rule (`crates/flui-rendering/src/error.rs` — every `RenderError` variant). Older crates (`flui-assets`, `flui-build`, `flui-cli`, `flui-engine`, `flui-painting`, etc.) still use `String` and are not retrofit targets for this PR — log them in the respective crate's `## Outstanding refactors` if the cost matters. New error types should adopt `Box<str>` from inception.

4. **`#[source]` on wrapping variants** (or `#[from]` which implies `#[source]`). Preserves the `Error::source()` chain for `Display`/`tracing` consumers. Use `#[from]` only when the conversion is the **only** way that variant is constructed; otherwise hand-write the constructor and use `#[source]` on the wrapped field.

5. **No `Clone` derive on errors.** Errors are terminal values — duplicated propagation indicates a code smell. The few sites that genuinely need to share an error use `Arc<RenderError>`; that is a per-site decision flagged in the call graph.

6. **`Result<T, E>` typed end-to-end in framework code.** Library crates never use `anyhow::Error` in public APIs. The dependency `anyhow = "1.0"` is in `[workspace.dependencies]` for **app-edge** use (`crates/flui-app`, binary `main`, example apps) — never for library APIs. If a library crate adds `anyhow::Error` to a public signature, that is the signal to introduce a typed `Error` for that crate.

7. **Never `Box<dyn Error>` in storage.** It is `!Copy`, heap-allocates, and loses variant information (no `match`). Storage form for cross-crate errors is the concrete `OuterError` with `#[from] InnerError` or a manual `From` impl. `Box<dyn std::error::Error + Send + Sync>` may appear at a single FFI boundary — flag with `// PORT-CHECK-OK-DYN: error trait erasure at FFI`.

8. **`build()` is infallible.** Per FOUNDATIONS.md C7: a panic inside user `View::build` is caught by `std::panic::catch_unwind` at the dispatch site and substituted with an `ErrorView` (the build-phase recovery surface — user-facing error widget). `RenderError::Poisoned` is the separate variant for panics inside render-object trait methods (`perform_layout` / `paint`) per `crates/flui-rendering/ARCHITECTURE.md ## Mapping decisions`. User code does not return `Result` from `build`; framework code does.

9. **Cross-crate error conversion** flows through `#[from]` or a hand-written `impl From<InnerCrateError> for OuterCrateError`. Never via `.map_err(|e| OuterError::Other(format!("{e}")))` — that loses the source chain.

### Anti-patterns

- `anyhow::Error` in a `pub fn` signature of a library crate — see rule 6.
- `Result<T, String>` — strings are not errors; they are messages on errors.
- `panic!` in framework code as the error-handling mechanism — `panic!` is reserved for invariant violations the type system cannot express. User-side panics are caught; framework-side panics are bugs.
- `Result<(), ()>` — `()` is not an error; use a typed enum even if it has one variant.

### When to introduce a new error type

A new error enum is justified when:
- The crate produces ≥2 distinct error conditions.
- A consumer needs to `match` on which condition fired (programmatic recovery, not just logging).
- The error crosses a public crate boundary.

A crate with one error condition and one consumer can use a unit struct (`struct MyError;`) implementing `Error` + `Display` — but the moment a second condition appears, switch to a `thiserror` enum.

---

## Inline port markers tier

Decisions made at the line level need to survive the journey from Phase A (mechanical port) to Phase B (idiom polish + perf comparison + soundness re-read). Per-crate `ARCHITECTURE.md ## Mapping decisions` covers crate-scope decisions; the four markers below cover line-scope decisions.

| Marker | When | Action expected |
|---|---|---|
| `// TODO(port): <reason>` | The Dart construct couldn't be translated confidently; the current Rust shape is a placeholder | Phase B re-reads the Dart, picks a translation, removes the marker. |
| `// PERF(port): <Dart idiom> — profile if hot` | Translated to the idiomatic Rust shape, but the Dart used a perf-specific construct (capacity hint, comptime mono, arena bulk-free) that the port elided in favour of clarity | Phase B greps `PERF(port)` and benchmarks the listed call sites; if the perf gap matters, restore the idiom (e.g., add `Vec::with_capacity`). |
| `// PORT NOTE: <reshape reason>` | The Rust shape diverged from the Dart shape on purpose — borrow-checker requires a reorder, Rust idiom is cleaner, or a typestate replaces a runtime check | Reviewer diff-reads `.dart` ↔ `.rs` side-by-side and uses the marker as the "expected divergence" anchor. |
| `// SAFETY: <invariant>` | Above every `unsafe` block — mirrors the unsafe code guidelines | Reviewer audits the invariant. Standard Rust practice, included here for completeness. **Not counted by `just port-markers`** — the unsafe audit is a separate concern from port-translation marker discipline. |

### Grammar

- Markers live on the line immediately above the affected expression, or as a trailing comment on the same line if the expression fits.
- The marker prefix is **exact** — `TODO(port)`, `PERF(port)`, `PORT NOTE`, `SAFETY` — case-sensitive, no abbreviations.
- The reason text after `:` is free-form, but must name **what** is deferred or reshaped (not "fix later"). Good: `TODO(port): late initialization needs OnceCell or Option — pick after build-context wiring lands`. Bad: `TODO(port): fix`.
- Markers may stack: `// TODO(port): ...` immediately above `// PORT NOTE: ...` is fine when both apply.

**Regex (used by `just port-markers`):** `\/\/\s+(TODO\(port\)|PERF\(port\)|PORT NOTE)` matched against every `.rs` file under `crates/`. Slashes escaped to survive MSYS2 bash path-mangling on Windows; no `\b` after `\)` because both `)` and `:` are non-word characters and the boundary would silently never fire. `SAFETY` is intentionally omitted (see table row above). When this grammar changes, update **both** this regex and `scripts/port-check.sh` `marker_pattern` in the same PR — the two must stay byte-identical or the script silently diverges from the doc.

### Worked examples

```rust
// PORT NOTE: Dart held the list as `late final List<int> ids = [];`; Rust uses
// OnceCell so reads-before-init panic with a typed error rather than the Dart
// LateInitializationError. Init point is `attach()`.
ids: OnceCell<Vec<ElementId>>,

// PERF(port): Dart used `List.filled(n, 0, growable: false)` (one alloc, no
// growth). Rust path collects into Vec; pre-size if profiling shows pressure.
let buf: Vec<u8> = source.bytes().collect();

unsafe {
    // SAFETY: `idx` is checked against `slab.len()` two lines above; the
    // disjoint-keys invariant for get_two_mut holds because parent != child.
    let (parent, child) = slab.get_two_mut(parent_id, child_id);
    ...
}

// TODO(port): Dart's `_dependents.add(WeakReference(element))` has no direct
// Rust analog — `std::rc::Weak` requires the owner to be `Rc<T>`, which the
// element arena is not. Hold a raw `ElementId` and validate on use.
self.dependents.push(element.id());
```

### `port-check` integration

`scripts/port-check.sh` exposes a **marker budget report** alongside the refusal-trigger checks. It does **not** fail on marker presence — markers are deliberate deferrals, not violations. It reports counts so Phase B can grep them when ready.

```text
just port-check               # refusal triggers only; markers reported in -v
just port-check-verbose       # adds per-trigger pass lines + marker totals
just port-markers             # per-file marker breakdown (TODO/PERF/PORT NOTE)
```

The marker grammar above is enforced by the grep — a `TODO` without `(port)` is a regular `TODO` (handled by clippy, not this script).

---

## Ecosystem-first principle

flui actively adopts the Rust ecosystem. Bun's PORTING.md bans `tokio`/`rayon`/`hyper`/`futures` and rolls its own primitives because Bun is a runtime — it owns its event loop and syscalls. flui is a UI framework — it sits **on top of** Rust's ecosystem and benefits from every mature crate it can absorb. **Reinventing the wheel is an anti-pattern.**

### Adopted ecosystem crates

The table below is the canonical "use this, don't write a custom one" lookup. Versions track `Cargo.toml` `[workspace.dependencies]`; see the file for the authoritative pin.

| Domain | Crate | Why not std / hand-rolled |
|---|---|---|
| Sync primitives | `parking_lot` | Faster, smaller, no lock poisoning. `parking_lot::Mutex` / `RwLock` are the workspace default; `std::sync::*` are used only when `Send`/`Sync` bounds or `MutexGuard` lifetime cross a tokio await point. |
| Concurrent map | `dashmap` | Shard-locked; no global contention. Use when a `HashMap` is read concurrently from multiple threads without a coarser external lock. |
| Inline-storage Vec | `smallvec` | Stack-fallback `Vec` for hot small-N (typical: `SmallVec<[T; 4]>` for child arrays). Avoids the heap allocation in the small case. |
| Delegation | `ambassador` | The Dart `with` mixin maps to `#[derive(Delegate)] #[delegate(Trait, target = "field")]`. Hand-written delegation is ~10x more code. |
| Builders | `bon` | Typestate-checked builder DSL — replaces hand-rolled `BuilderContextBuilder<P, Pr>` boilerplate. |
| Errors | `thiserror` | Derive macros for `Error` + `Display` + `source()`. See [§Error mapping canonical shape](#error-mapping-canonical-shape). |
| App-edge errors | `anyhow` | Boxed error for binary `main` and ad-hoc tooling — **never** in library APIs (rule 6 in §Error mapping). |
| Logging | `tracing` + `tracing-forest` | Span-aware structured logging. `tracing::debug!(?value)` lazily formats only when the subscriber accepts. |
| Hashing | `ahash` | Faster than std `SipHash` for non-DoS data. Workspace-default hasher (see §Type map — `Map`, `Set`). |
| Caching | `moka` | Concurrent LRU/TLRU with future-aware loaders. Used by `flui-assets`. |
| GPU | `wgpu` (29.x) | The cross-platform GPU abstraction. flui-engine sits directly on wgpu; no intermediate (no Skia, no custom backend). |
| Windowing | `winit` (0.30) | Cross-platform window + event loop. |
| Image decoding | `image` (PNG/JPEG/GIF; WebP deferred per upstream fix for Rust 1.91+) | Standard image-decoding crate. |
| Font parsing | `ttf-parser` | Lightweight, no allocation. |
| Async runtime | `tokio` (1.43 LTS) | **Edges only** — Refusal trigger 3. Pinned to the LTS line for stability. |
| HTTP client | `reqwest` | Async HTTP, rustls TLS. `flui-assets` only. |
| UI events | `ui-events` + `ui-events-winit` | W3C-compliant cross-platform input abstraction. |
| Pointer-projection / pin | `pin-project-lite` | Light pin-projection without proc-macro cost. |
| Cancellation / signaling | `crossbeam` | Lock-free channels and atomics. |
| One-shot init | `once_cell` (kept) + std `LazyLock`/`OnceLock` (Rust 1.80+) | Use std forms when the bound supports it; fall back to `once_cell::sync::Lazy` only when the init signature does not fit. |

When a need arises that the table does not cover, the order of operations is:

1. Check if a crate already in the workspace can be repurposed.
2. Search [crates.io](https://crates.io) and read the candidate's README + recent issues; check `crates.io` health (last release date, dependency count, reverse dependencies — `mcp__cratesio__crate_health_check` is available).
3. Add to `[workspace.dependencies]` with the **caret-pinned** latest stable; document the addition in a `## Mapping decisions` entry of the first consumer crate.
4. Hand-rolling is the last resort — and requires a `## Mapping decisions` entry explaining why no crate fits.

### Version policy

- **Rust toolchain**: pinned to an explicit stable version (currently `channel = "1.96.0"`) in [`rust-toolchain.toml`](../rust-toolchain.toml), kept in lockstep with the MSRV below and bumped by the same PR.
- **MSRV** (`rust-version` in [`Cargo.toml`](../Cargo.toml)): bumped **no later than 6 weeks** after a new stable release. Rust ships every 6 weeks (current: **1.96**, released 2026-05-25; next: **1.97** ~2026-07-09). The MSRV bump PR is mechanical — bump the field and `rust-toolchain.toml`, update the CI matrix, ship.
- **Workspace dependencies**: caret-pinned (`"1.43"`, not `"=1.43.2"`). Patch bumps automatic via `cargo update`. Minor bumps batched monthly; major bumps reviewed individually.
- **Pinned exceptions**: documented inline in `Cargo.toml` (current: `image` `webp` feature disabled per `image-webp` issue #102; no wgpu pin — tracking latest stable major).

### Recent stabilizations to fold into the port

Rust 1.95 (2026-04-16) introduced features directly relevant to this port:

| Feature | Where it lands in the port |
|---|---|
| `cfg_select!` macro | Replaces paired `#[cfg(...)]` + `#[cfg(not(...))]` blocks for platform conditionals. See [§Dart → Rust idiom map](#dart--rust-idiom-map) row "compile-time-conditional code". |
| `if let` guards in `match` | Sharpens the Dart `is Foo` + downcast pattern: `match x { Foo(v) if let Some(inner) = v.maybe_extract() => ... }`. See [§Dart → Rust idiom map](#dart--rust-idiom-map) row "switch + pattern guard". |
| `AtomicBool/Isize/Usize/Ptr::update` and `try_update` | Encapsulates the standard `load → compute → compare_exchange_weak` loop. Candidate cleanup sites in `crates/flui-rendering/src/storage/state.rs` (`AtomicRenderFlags` CAS loops); mark with `// PERF(port): pre-1.95 CAS loop`. |
| `Vec::push_mut` / `insert_mut`, `VecDeque::push_front_mut` / `push_back_mut`, `LinkedList::push_front_mut` / `push_back_mut` | Returns `&mut T` to the inserted slot — useful for chained init like `vec.push_mut(Node::new()).attach(parent_id);`. `LinkedList::insert_mut` was **not** stabilized in 1.95. |
| `Layout::dangling_ptr` / `repeat` / `repeat_packed` / `extend_packed` | Allocator primitives. Phase B hot-path candidates if/when flui adopts an arena allocator beyond `bumpalo` (currently not a workspace dep). |

Rust 1.96 (~2026-05-28) stabilizations relevant to this port will be added here as they are confirmed and applied. Rust 1.97 will be tracked in this section on release.

---

## Don't translate

Some Dart constructs do **not** map to Rust at all. Listing them up front prevents one-way translation effort on code that should be deleted or replaced wholesale.

### Source-level dropped

- **`import 'package:flutter/...';`** lines — Rust uses `use` at the top of each module; do not 1:1 the Dart import block. The Rust file's import surface is shaped by what it actually uses, not by what the Dart file declared.
- **Test files mirroring Flutter's `test/` layout** — Rust has `#[cfg(test)] mod tests { #[test] fn ...() {} }` inline in the source file (preferred for unit) or `tests/<name>.rs` (for integration). Do not create `crates/<crate>/test/` directories.
- **`mockito` / `flutter_test` fixtures** that are pure Dart-API ceremony — Rust uses `#[cfg(test)]` modules and `mockall` / hand-written fakes; the fixture surface is different.
- **Generated `.g.dart` files** (json_serializable, freezed, etc.) — translate the *intent* (data class, serde derive) rather than the generated output. Mark with `// PORT NOTE: was generated from <generator>`.
- **`pubspec.yaml`** — replaced by `Cargo.toml`. Do not translate dep lines 1:1 — see [§Ecosystem-first principle](#ecosystem-first-principle) for the workspace-default mapping.

### Deleted, not ported (binding-deletion carve-out)

A Flutter binding may be **deleted**, not ported, when a Rust-native crate stack already owns the responsibility end-to-end. This is the [§Mapping rules](#flutter-behaviour-primacy-with-binding-deletion-carve-out) carve-out. Recorded precedents:

| Dart construct | Replaced by | Recorded in |
|---|---|---|
| `PlatformTextSystem` (Flutter text-shaping abstraction) | `cosmic-text` + `glyphon` + `flui-assets` text stack | [`docs/plans/2026-03-31-platform-roadmap.md`](plans/2026-03-31-platform-roadmap.md) Task 1 |
| `LayerHandle<T>` (Flutter cached-layer pointer, 467 LOC + 17 aliases, 0 external callers) | deleted; layer caching handled at the `flui-layer` enum + `LayerId` level | [`crates/flui-layer/ARCHITECTURE.md`](../crates/flui-layer/ARCHITECTURE.md) Mythos Step 1 |
| `ShaderWarmUp` (Flutter Skia shader pre-compilation hook) | deleted; wgpu compiles pipelines on first use, no warm-up phase | [`crates/flui-painting/ARCHITECTURE.md`](../crates/flui-painting/ARCHITECTURE.md) |

When proposing a new binding-deletion, the three conditions in [§Mapping rules](#flutter-behaviour-primacy-with-binding-deletion-carve-out) apply: end-to-end Rust-native ownership, no Flutter-semantic break, and a `## Mapping decisions` entry citing the precedent table above.

### Not yet replaced — leave for Phase B

When a Dart construct has no obvious Rust mapping and no precedent for deletion, the right move is a `// TODO(port)` marker, not a forced translation. Examples:

- **`Isolate.spawn` with closed-over state** — Dart isolates have no shared heap; Rust threads share via `Arc`. The translation depends on what the isolate was *for* (CPU work → `tokio::spawn_blocking`; UI thread isolation → already implicit in Rust's sync hot path).
- **Dart mirrors / `dart:mirrors`** — runtime reflection, removed from Flutter long ago. Any source still using it indicates dead code; verify via the Flutter Master branch and delete.
- **Dart FFI auto-generated bindings** (`dart:ffi`) — replaced by hand-written `extern "C"` blocks against the same C ABI; do not translate the FFI scaffolding.

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
- **`## Marker budget`** — when the crate carries more than a handful of `TODO(port)` / `PERF(port)` / `PORT NOTE` markers, list them here as a Phase B work-queue. `just port-markers` produces the per-file breakdown that seeds this section.

### Graft instructions for existing docs

Three crates already hold port-flavoured documents that predate this template:

- [`crates/flui-foundation/ARCHITECTURE.md`](../crates/flui-foundation/ARCHITECTURE.md) — Flutter-walk + Architecture Decision Summary; graft by appending the missing `## Thread safety` / `## Friction log` / `## Outstanding refactors` sections; do not rewrite the existing body.
- [`crates/flui-rendering/flutter-rendering-hierarchy.md`](../crates/flui-rendering/flutter-rendering-hierarchy.md) — 1352-LOC Flutter class hierarchy dump; remains as a sibling appendix linked from the templated `crates/flui-rendering/ARCHITECTURE.md`.
- [`crates/flui-view/UNIFIED_ELEMENT.md`](../crates/flui-view/UNIFIED_ELEMENT.md) — element behaviour taxonomy; remains as a sibling appendix linked from the future templated `crates/flui-view/ARCHITECTURE.md` when that crate is templated.

Four other crates carry `crates/<crate>/docs/ARCHITECTURE.md` files (`flui-painting`, `flui-interaction`, `flui-animation`, `flui-assets`). Relocating those to the crate root per the `AGENTS.md` convention is deferred to a follow-up doc-tidying PR; this methodology does not require relocation upfront.

---

## Index

This section indexes **crate-level** `ARCHITECTURE.md` template state. For document-level section navigation see [§Contents](#contents) at the top of this page.

| Crate | `ARCHITECTURE.md` state | Status |
| --- | --- | --- |
| [`flui-foundation`](../crates/flui-foundation/ARCHITECTURE.md) | Templated (grafted 2026-05-19) | Active |
| [`flui-rendering`](../crates/flui-rendering/ARCHITECTURE.md) | Templated 2026-05-20 (exemplar instance, U2 refactor recorded) | Active |
| `flui-geometry` | Not yet templated | Active |
| `flui-types` | Not yet templated | Active |
| `flui-tree` | Not yet templated | Active |
| `flui-macros` | Not yet templated | Active |
| `flui-platform` | Not yet templated | Active |
| [`flui-painting`](../crates/flui-painting/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active |
| `flui-semantics` | Not yet templated | Active |
| `flui-scheduler` | Not yet templated | Active |
| [`flui-layer`](../crates/flui-layer/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active |
| `flui-interaction` | `crates/flui-interaction/docs/ARCHITECTURE.md` (pre-template; precedent for `## Thread safety` format) | Active |
| [`flui-engine`](../crates/flui-engine/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active |
| `flui-hot-reload` | Not yet templated | Active |
| `flui-objects` | Not yet templated | Active |
| `flui-view` | `crates/flui-view/UNIFIED_ELEMENT.md` (companion; not templated) | Active |
| `flui-widgets` | Not yet templated | Active |
| `flui-binding` | Not yet templated | Active |
| `flui-app` | Not yet templated | Active |
| `flui-animation` | `crates/flui-animation/docs/ARCHITECTURE.md` (pre-template) | Active |
| `flui-reactivity` | Not yet templated | Disabled |
| `flui-devtools` | Not yet templated | Active |
| `flui-cli` | Not yet templated | Active |
| `flui-build` | Not yet templated | Active |
| `flui-assets` | `crates/flui-assets/docs/ARCHITECTURE.md` (pre-template) | Active |

Authoritative workspace state lives in [`AGENTS.md`](../AGENTS.md) and [`docs/crates.md`](crates.md); this index restates "templated yes/no" only.

External references this methodology builds on:

- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — v2.2.0 anti-patterns and architectural rules.
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full anti-pattern list with code examples.
- [`STRATEGY.md`](../STRATEGY.md) — port rationale, Bun precedent, three architectural clauses.
- [`docs/plans/2026-03-31-core-crates-hardening.md`](plans/2026-03-31-core-crates-hardening.md) — `Weak<RwLock<PipelineOwner>>` precedent.
- [`docs/plans/2026-03-31-platform-roadmap.md`](plans/2026-03-31-platform-roadmap.md) — `PlatformTextSystem` deletion precedent (source of the binding-deletion carve-out).
- [`docs/plans/2026-03-31-custom-render-callback-design.md`](plans/2026-03-31-custom-render-callback-design.md) — canonical justified `Box<dyn>` exception template.
- [`docs/plans/2026-03-31-engine-hardening.md`](plans/2026-03-31-engine-hardening.md) — multi-source reference precedent (GPUI, Makepad, Iced, Vello, Skia).
- [Bun PORTING.md](https://github.com/oven-sh/bun/blob/main/docs/PORTING.md) — the structural inspiration for §Dart→Rust type/idiom maps and the marker-tier convention. flui inverts Bun's "no ecosystem" stance — see [§Ecosystem-first principle](#ecosystem-first-principle).

---

## Verification

The `just port-check` recipe runs the refusal-trigger regexes from this document against the workspace and exits non-zero on any match outside the whitelist. Run it before opening a PR that touches `flui-rendering`, `flui-view`, or `flui-painting`:

```text
just port-check               # silent on pass; lists each violation on fail
just port-check-verbose       # prints "ok" lines for each passing trigger + marker totals
just port-markers             # per-file marker breakdown (TODO(port) / PERF(port) / PORT NOTE)
```

The underlying script lives at [`scripts/port-check.sh`](../scripts/port-check.sh). It runs one `rg` (ripgrep) pass per trigger — 21 refusal triggers plus the FR-033 downcast grep and the FR-036 sanctioned-`dyn`-boundary registry (main pattern + type-alias closure) — and filters out doc-comment matches. The marker-budget scan is an additional non-blocking pass in `-v` and `-b` modes. The regexes are derived directly from the trigger entries in this document; when a trigger changes here, the script changes too.

The marker-budget report is a **non-blocking** addition: it counts `TODO(port)`, `PERF(port)`, and `PORT NOTE` occurrences across `crates/` and prints a per-crate summary. Markers are deliberate deferrals (Phase B work-queue), not violations — the script never fails on marker count.

**Cross-platform note.** The script is bash. On Windows, run via Git Bash or WSL — both ship with `bash` and modern `rg` on PATH. A PowerShell sibling is not provided in this iteration because the regex set is identical and dual-maintenance is not warranted at solo-maintainer scale.

The recipe is part of `just ci` and the GitHub `checks` job. When a trigger is promoted to a clippy lint per the [Reactive lint promotion](#reactive-lint-promotion) rule, that lint runs under `cargo clippy --workspace --all-targets -- -D warnings`; the doc entry stays as the human-readable surface for the rule.

### Self-test (negative-test confirmation)

`port-check` itself can be exercised by introducing a deliberate violation in a scratch file (e.g., `crates/flui-rendering/src/__port_check_scratch.rs` with a `RwLock<Box<dyn RenderObject<P>>>` field) and confirming the recipe exits non-zero, names the file and line, and references the correct trigger ID. Delete the scratch file when done. This confirms the recipe still distinguishes real violations from whitelist entries; do it after any change to the trigger regexes or the whitelist file globs.

The marker-budget pass can be self-tested by adding a `// TODO(port): scratch` line in a scratch file and confirming `just port-markers` lists the file and the marker count rises by one.

---

[← Architecture](architecture.md) · [Back to README](../README.md) · [Crates Map →](crates.md)
