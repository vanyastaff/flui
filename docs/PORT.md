[← Architecture](architecture.md) · [Back to README](../README.md) · [Crates Map →](crates.md)

# Port Methodology

FLUI is a **port** of Flutter's three-tree architecture into Rust, not a redesign. This page is the working methodology for that port — the rules the maintainer refuses to break at write time, the per-crate documentation shape that records the port decisions, and the index that pins which crate holds which mapping.

For the rule-by-rule architectural guide (workspace layers, anti-pattern code examples, dependency DAG), read [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md). For the strategic frame and "behavior loyal, structure Rust-native" rationale, read [`STRATEGY.md`](../STRATEGY.md). This page does not restate those; it is the operational layer that hangs off them.

---

## Refusal triggers

Each trigger is a rule the maintainer refuses to introduce. A violation found in review is the signal to either refactor the violating site or, if the same pattern is caught again, promote the trigger to a compile-time lint per the [Reactive lint promotion](#reactive-lint-promotion) rule below.

Triggers are seeded from observed friction in the workspace. Forward-looking triggers have zero current production violations and are enforced on introduction.

### 1. `RwLock` field on a type used inside `perform_layout` or `paint`

**Why:** the render hot path is strictly synchronous (see [`STRATEGY.md`](../STRATEGY.md) clause "sync hot path, async на краях"). A lock on a per-node storage type held across `perform_layout` or `paint` serialises the pipeline against itself and removes the "many readers OR one writer" guarantee the hot path depends on. Shared infrastructure locks (`PipelineOwner`, `WidgetsBinding`, route plumbing) are different — they sit one level above per-node mutation and are covered in [Lock decisions](#lock-decisions).

**Back-references:** [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) v2.2.0 Anti-Patterns ("`Arc<Mutex<>>` for tree structures"); [`STRATEGY.md`](../STRATEGY.md) "sync hot path".

**Regex (used by `just port-check`):** `RwLock<\s*Box<\s*dyn\s+(RenderObject|Layer\b|ContainerLayer)` (storage-shaped violations). Scope extended in Mythos Step 13 of the `flui-layer` chain to cover `crates/flui-layer/src/` and to match `dyn Layer` / `dyn ContainerLayer` shapes as well.

### 2. `Box<dyn RenderObject<_>>` wrapped in any interior-mutability primitive in render storage

**Why:** owned `Box<dyn RenderObject<_>>` stored as a plain field is acceptable — it is the chosen post-U2 baseline that preserves the open-set trait (blanket `impl<T: RenderBox + Diagnosticable> RenderObject<P> for T`) while delegating mutation discipline to the borrow checker through `&mut RenderTree`. The actual hazard is **wrapping** the trait object in any interior-mutability primitive (`RwLock`, `Mutex`, `RefCell`, `Cell`, `UnsafeCell`) on the storage type, because that would re-introduce the lock-or-interior-mutability problem the U2 refactor removed (the canonical violation was `RwLock<Box<dyn RenderObject<P>>>`).

Trigger 1 catches the specific `RwLock` variant. Trigger 2 catches any other interior-mutability wrap that would smuggle the same problem back in under a different primitive.

The *funnel* signatures (`tree.rs::insert_box`, view → render `From` impls) accept `Box<dyn RenderObject<_>>` as a transient parameter type and are not the target — the violation is the stored-and-wrapped shape.

**Back-references:** [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) example "`RenderBad { children: Vec<Box<dyn RenderObject>> }` — forbidden"; [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) Principle IV.

**Regex:** `(RwLock|Mutex|RefCell|Cell|UnsafeCell)<\s*Box<\s*dyn\s+(RenderObject|Layer\b|ContainerLayer)` constrained to render-storage modules and `crates/flui-layer/src/`. Scope and trait-name set extended in Mythos Step 13 of the `flui-layer` chain.

### 3. `async fn` on `View::build`, `RenderObject::layout`, `RenderObject::paint`

**Why:** the same sync-hot-path clause. Async on these methods would force the scheduler to await within a frame budget critical path.

**Back-references:** [`STRATEGY.md`](../STRATEGY.md) "sync hot path, async на краях"; permitted at IO (`flui-assets`), scheduler (`flui-scheduler`), build pipeline (`flui-build`) only.

**Regex:** `async\s+fn\s+(build|layout|paint|perform_layout|composite|render|fire_composition_callbacks)\b` constrained to `crates/flui-{rendering,view,painting,layer}/src/**`. Scope and verb set extended in Mythos Step 13 of the `flui-layer` chain to catch layer-level async (`composite`, `render`, `fire_composition_callbacks`).

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
| `flui-engine` | Not yet templated | Active |
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

The underlying script lives at [`scripts/port-check.sh`](../scripts/port-check.sh). It runs six `rg` (ripgrep) invocations — one per refusal trigger — and filters out doc-comment matches. The regexes are derived directly from the trigger entries in this document; when a trigger changes here, the script changes too.

**Cross-platform note.** The script is bash. On Windows, run via Git Bash or WSL — both ship with `bash` and modern `rg` on PATH. A PowerShell sibling is not provided in this iteration because the regex set is identical and dual-maintenance is not warranted at solo-maintainer scale.

The recipe is not part of `just ci` by default — refusal triggers are a write-time guard, and CI carries the lint baseline plus the test suite. When a trigger is promoted to a clippy lint per the [Reactive lint promotion](#reactive-lint-promotion) rule, that lint runs under `cargo clippy --workspace -- -D warnings` (already in `just ci`); the doc entry stays as the human-readable surface for the rule.

### Self-test (negative-test confirmation)

`port-check` itself can be exercised by introducing a deliberate violation in a scratch file (e.g., `crates/flui-rendering/src/__port_check_scratch.rs` with a `RwLock<Box<dyn RenderObject<P>>>` field) and confirming the recipe exits non-zero, names the file and line, and references the correct trigger ID. Delete the scratch file when done. This confirms the recipe still distinguishes real violations from whitelist entries; do it after any change to the trigger regexes or the whitelist file globs.

---

[← Architecture](architecture.md) · [Back to README](../README.md) · [Crates Map →](crates.md)
