# Feature Specification: Core Contracts — Heterogeneous Children + Widget-Authoring API + View / Element / Reconciliation (C2 + C3 + C4 + C6)

**Feature Branch**: `004-view-element-core` *(historical name — originally scoped as C4+C6; expanded per the 2026-05-22 doc-review finding that the four contracts cannot be locked independently)*
**Created**: 2026-05-22
**Last revised**: 2026-05-22 (round 1 doc-review revision)
**Status**: Draft (round-2)
**Input**: Lock the FLUI Core Contracts — heterogeneous children (C2), widget-authoring API (C3), and the View / Element / Reconciliation core (C4 + C6) from `docs/FOUNDATIONS.md` Part III. Co-designed as one atomic merge unit because the four contracts share files and propagate constraints across each other (the `ViewSeq` shape forces the reconciler signature; `impl IntoView` ergonomics force the authoring surface; the element-storage shape couples to the heterogeneous-children boundary).

---

## Context

This specification locks the **bottom of the widget-authoring contract** — the surface every future widget in `flui-widgets`, `flui-material`, and `flui-cupertino` will commit to at its first line. It covers four FOUNDATIONS clauses from `docs/FOUNDATIONS.md` Part III, locked together as one atomic merge unit:

- **C2** — Heterogeneous children: a `ViewSeq` trait with two equally load-bearing paths (static tuple + dynamic `Vec<BoxedView>`).
- **C3** — Widget-authoring API: `View::build`-equivalent returns `impl IntoView`, `#[derive(StatelessView)]` removes boilerplate, `bon` builders for many-field constructors.
- **C4** — `View` trait surface (object-safe, no public lifetime) + element storage (closed enum over the behavior set, deals correctly with `Arity` parameter and `AnimationBehavior` composition).
- **C6** — Keyed reconciliation algorithm (Flutter's linear O(N) keyed update, every `ElementNode` carrying a `key` field).

**Why unified.** Round 1 of the doc-review found a hard sequencing problem: three of the four contracts touch the same `flui-view` files and propagate constraints across each other.

- `ViewSeq` (C2) forces the reconciler's contiguous-fast-path signature (C6) and the element-storage child shape (C4).
- `impl IntoView` (C3, C4) forces the public surface every widget commits to (C2, C4).
- The element-storage enum (C4) must accommodate both the tuple-static path and the dynamic-fallback path (C2).

ROADMAP Core.0 originally listed "three design docs … in parallel." Three parallel docs would still need to be merged together to avoid the sequencing risk; one unified spec achieves the same with less ceremony and prevents cross-spec references that the FRs themselves cross.

**Current code is in a fragile half-applied state.** A typed `IntoView` trait exists alongside `build()` methods that still return `Box<dyn View>`. A correct keyed `reconcile_children` exists in the codebase but its keyed middle section is a *stub* (`reconciliation.rs:91-98`: `// This would need enhancement to store keys in ElementNode / let _ = (i, node);`), its tests cover only zero-key cases, and element storage is `Box<dyn ElementBase>` with a runtime `downcast_ref::<V>()` that silently logs a warning on mismatch. Heterogeneous children rely on `Children` (`Vec<BoxedView>`) with builder-only construction, `dyn_clone` per frame, and no array-literal path. Widget-authoring is a 3-step ritual (`struct` + manual `impl View` + `impl_stateless_view!` macro), with `bon` unused despite being the constitution's stated builder dependency. Locking these four contracts together is a precondition for the Core.1 vertical slice — ROADMAP Core.0 owns this work.

**Audience.** Three user types touch this contract:

- **Widget authors** — anyone writing a `View` implementation (StatelessView / StatefulView / ViewState) in `flui-widgets`, `flui-material`, `flui-cupertino`, or an end-user app. The dominant audience. They author with `#[derive(StatelessView)]`, return child trees via `impl IntoView`, compose heterogeneous children via `column!`/`row!` macros or `Vec<BoxedView>` for dynamic counts.
- **End-application developers** — those who *compose* widgets (from the catalog, from third parties) into apps without writing `impl View` themselves. They never touch `IntoView`, `Key`, or `ElementKind` directly, but they observe the consequences of every contract in this spec — keyed reorder preservation, theme propagation, dynamic-list ergonomics.
- **Framework contributors** — those extending the element-tree machinery (a new element-behavior variant, a new reconciler path, devtools instrumentation). They need closed-set discipline so adding behaviors is mechanically discoverable, and failures must be loud (not `tracing::warn!` + silent stale state).

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — A reordered keyed list preserves item state (Priority: P1)

A widget author builds a list of stateful items (a chat thread, a todo list, a sortable table). Each item carries a `Key` derived from its identity. The user reorders the list. After the reorder, every item is in its new position with all per-item state (scroll offset, animation progress, focus, expanded/collapsed flag) intact. No item flickers, re-fetches its data, or loses its place.

**Why P1**: this is the silent-correctness trap the current code falls into. Today every list / grid / table loses item state on reorder, but it never fails a test because static demos never reorder. Shipping the widget catalog on this defect would bake the bug into every list-shaped widget in `flui-material` and `flui-cupertino`, surfacing only in production user apps.

**Independent Test**: a widget author writes a `Variable`-arity multi-child widget with three keyed children, swaps the order, and asserts per-child state is preserved at the new position.

**Acceptance Scenarios**:

1. **Given** a tree with `[A(key=k1), B(key=k2), C(key=k3)]` where each child holds a per-item counter at 0, **When** the parent rebuilds with `[C(key=k3), A(key=k1), B(key=k2)]`, **Then** each child's counter remains at 0 (state preserved, no remount).
2. **Given** `[A(key=k1), B(key=k2)]` and A's counter incremented to 5, **When** the parent rebuilds with `[B(key=k2), A(key=k1)]`, **Then** A's counter is still 5 at its new position.
3. **Given** mixed keyed and unkeyed children `[A(no key), B(key=k2), C(no key)]`, **When** the parent rebuilds with `[A(no key), C(no key), B(key=k2)]`, **Then** B is moved to its keyed slot and the unkeyed children fall back to positional matching.
4. **Given** a child with `GlobalKey<W>(K)` mounted under parent P1, **When** the next rebuild places that same `GlobalKey<W>(K)` child under a different parent P2, **Then** the child's element is reparented (not re-mounted) — state survives. The reparenting flows through the new keyed reconciler, not via a side-channel registry bypass (see FR-029).

---

### User Story 2 — Static heterogeneous children (Priority: P1)

A widget author writes `Column { children: column![Text("a"), Button { label: "b" }, Image::asset("c.png")] }` — a column of three heterogeneous children. The expression compiles without the author writing `Box::new`, `.into_view()`, `vec![Box::new(...)]`, or any `dyn` syntax. Each child keeps its concrete type to the `Slab` boundary; the reconciler's contiguous-update fast path is monomorphic per child position.

**Why P1**: `children` is the spine of every multi-child widget (`Column`, `Row`, `Stack`, `Wrap`, `Flex`, `Table`). This is the most-touched authoring surface in the framework — every example in the docs, every widget in `flui-material` that takes children. If the catalog ships on builder-only `Children` syntax, every public example reads worse than its Flutter twin and `STRATEGY.md`'s "external contributor mental model legible from outside" metric is suppressed at the source.

**Independent Test**: a widget author writes a `Column` literal with three children of three different `View` types using the `column!` macro; the code compiles without explicit `Box`/`dyn` syntax in the author's source.

**Acceptance Scenarios**:

1. **Given** the macros `column!` / `row!`, **When** an author writes `column![Text("a"), Padding { child: Text("b"), padding: EdgeInsets::all(8.0) }, GestureDetector { on_tap: ..., child: Text("c") }]`, **Then** the expression compiles and `Column { children: ... }` accepts the result. The author's source contains no `Box::new`, no `.into_view()`, no `vec!`.
2. **Given** a tuple `(text, button, image)` of heterogeneous `View` values, **When** assigned as `children` to a multi-child widget, **Then** the tuple implements `ViewSeq` via a macro-generated impl for arities `0..=16`. Each child's concrete type is preserved to the element boundary.
3. **Given** a static `column!`-built widget tree, **When** the reconciler runs in a benchmark, **Then** the contiguous-update fast path uses monomorphic per-position dispatch (no `dyn`-call overhead measured by `cargo-asm` on the inner update loop).

---

### User Story 3 — Dynamic heterogeneous children (Priority: P1)

A widget author writes `ListView { children: items.iter().map(|item| build_row(item).boxed()).collect::<Vec<BoxedView>>() }` — a list of N children built from runtime data. The `Vec<BoxedView>` path is **first-class**, not a rare fallback. Every scrolling and data-display widget in the catalog (`ListView`, `GridView`, `CustomScrollView`, `DataTable`, every `Vec`/iterator-driven widget, much of Material) sits on this path.

**Why P1**: state preservation under list reorder applies to dynamic-children lists just as much as to static tuples. Treating `Vec<BoxedView>` as a "rare fallback" and skipping it in the test corpus would ship a silent-correctness trap on every list/grid/scroll widget — the same defect class US1 closes. The dynamic path's quantitative share of the catalog (single-child `child:` slots ~100× more common than `List<Widget> children` in Flutter Material) does not change the priority: any dynamic list that does exist is high-visibility (scrollables are what users see), and the silent-correctness symmetry with US1 is the real P1 driver. **Sequence note**: US3 builds on US2's algorithm — US2 ships the keyed reconciler against the tuple-static path first; US3 reuses the same reconciler on the dynamic path. If scope pressure forces deferring one, defer US3 (not US2) — the algorithm is the shared piece and US2 exercises it first.

**Independent Test**: an author builds a dynamic-count `ListView` from a `Vec` of differently-typed items (some `Text`, some `Image`, some `Card`) using `.boxed()` per item; the list renders correctly, reorders preserving keyed state (see US1 acceptance scenarios applied to the dynamic path), and the keyed reconciler is exercised on the dynamic path identically to the static path.

**Acceptance Scenarios**:

1. **Given** a `Vec<BoxedView>` of 1,000 heterogeneous keyed children, **When** the parent rebuilds with the same Vec reordered, **Then** each child's per-item state is preserved (US1's acceptance applied to the dynamic path).
2. **Given** a conditional widget `if loading { Spinner::default().boxed() } else { Content::for(data).boxed() }` returning different `View` types, **When** the conditional is invoked from inside `build()`, **Then** both branches compile and produce a `BoxedView` value the framework accepts.
3. **Given** a `Vec<BoxedView>` of N children and a `Vec<BoxedView>` of N+M children (M items added at the tail), **When** the parent rebuilds, **Then** the existing N children's elements are reused (no remount) and only the M new items are mounted.

---

### User Story 4 — Clean widget-authoring (Priority: P1)

A widget author writes a new custom widget. They write the smallest possible amount of code: a struct holding parameters, `#[derive(StatelessView)]` for the trivial case (or `#[derive(StatefulView)]` for the stateful one), and one `build()` method that returns a child tree. Optional: `bon` builders for many-field widgets give a fluent constructor. No `Box::new`, no manual `impl View` for the common case, no ritual.

**Why P1**: the most-touched public surface in the entire framework. Every widget ever written sits here. `STRATEGY.md`'s adoption metric depends on this being parity-with-or-better-than Flutter; a verbose authoring API suppresses the metric at the source. The current 3-step ritual (`struct + impl View + impl_stateless_view!`) is acceptable but unergonomic; the `bon` dependency is declared in the workspace but unused for widgets.

**Independent Test**: a widget author writes the trivial `Greeting { name: String }` stateless widget and the parallel Flutter `class Greeting extends StatelessWidget` — the FLUI source has ≤ 6 lines of widget-author code (matching or beating Flutter's line count) and contains no `Box::new`/`dyn`/`impl_stateless_view!` macro invocation in the author's surface.

**Acceptance Scenarios**:

1. **Given** the `#[derive(StatelessView)]` proc-macro, **When** an author writes `#[derive(Clone, StatelessView)] struct Greeting { name: String } impl Greeting { fn build(&self, ctx: &dyn BuildContext) -> impl IntoView { Text::new(&self.name) } }`, **Then** the code compiles, `Greeting` is a usable `View`, the source contains no `Box::new` / no `impl View for Greeting` block / no `impl_stateless_view!` invocation.
2. **Given** a widget with N>3 constructor fields, **When** the author adds `#[derive(::bon::Builder)]`, **Then** the call site `Card::builder().title("t").body("b").elevation(2).build()` compiles and is preferred over a positional N-argument constructor for clarity.
3. **Given** a stateful widget, **When** the author writes `#[derive(StatefulView)] struct Counter { initial: u32 }` and an associated `impl ViewState for CounterState { type View = Counter; fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView { ... } }`, **Then** the state-handle machinery is wired and `ctx.set_state(|s| s.count += 1)` works without the author writing an `impl View for Counter` block.

---

### User Story 5 — Framework dispatch is typed, no silent downcast (Priority: P1)

When a parent passes a `View` to a child element during reconciliation, the dispatch is a typed `match` on the closed `ElementKind` enum — no `downcast_ref::<V>()`, no `tracing::warn!` on type mismatch, no silently-dropped update. Type mismatches replace the element (Flutter-correct behavior). End-user widget composition errors fail loudly or unambiguously.

**Why P1** (re-prioritized from P2 per doc-review finding #10): the current `downcast_ref::<V>()` in the update path (`generic.rs:271`) is the textbook "unknown unknown" defect. When it fails, production logs a warning and continues with stale state. It cannot fail in a test where types are known statically; it surfaces only when end-users compose widgets in a way the test didn't anticipate. This is the symmetric silent-correctness claim to US1 — a widget-author *reliability* claim, not a framework-contributor convenience.

**Independent Test**: an integration test composes a parent that passes the wrong widget type to a child position; the test asserts the framework replaces the element (correct Flutter behavior) without emitting any `tracing::warn!` related to type cast failure.

**Acceptance Scenarios**:

1. **Given** an element of one behavior receives a `View` of an incompatible type during reconciliation, **When** the reconciler dispatches the update, **Then** the framework replaces the element. No `tracing::warn!` related to a failed type cast is emitted by the update path.
2. **Given** the framework runs a full build / layout / paint frame in CI under all 8 `port-check.sh` triggers (7 existing + the new `downcast_ref`-in-update-path grep), **When** the frame completes, **Then** the trigger grep matches zero occurrences of runtime `downcast_ref::<V>()` in the View-type update dispatch path (legitimate non-View-type uses are whitelisted per FR-033).

---

### User Story 6 — Discoverable element-behavior extension (Priority: P2)

A framework contributor extends the element tree with a new behavior variant (a future devtools-instrumentation variant, say). They add it to the closed `ElementKind` enum, run `cargo build`, and the compiler enumerates every site that has to handle the new variant. There is no runtime branch that silently does the wrong thing for the new behavior.

**Why P2** (not P1): this is a framework-contributor convenience, not an end-user-visible reliability claim (US5 covers the user-visible half). It is genuinely important — closed-set discipline is what keeps the framework extensible without silent failures — but the priority is set by the contributor consequence, not the widget-author consequence.

**Independent Test**: a contributor adds a stub `Debug` variant to `ElementKind`, runs `cargo build`, and the compiler reports every non-exhaustive `match`. Recorded as a manual test in the spec's test plan.

**Acceptance Scenarios**:

1. **Given** the `ElementKind` enum is `#[non_exhaustive]` and closed, **When** a contributor adds a new variant, **Then** `cargo build` fails on every non-exhaustive `match` until each is handled.

---

### Edge Cases

- **Mixed keyed and unkeyed children.** Keyed children matched by `ViewKey::key_eq` (semantic equality on hash hit, mirroring Flutter), unkeyed children fall back to positional matching in the remaining slots.
- **Two children with the same `Key`.** Treated as Flutter does: `debug_assert!` failure in debug builds with a descriptive message; well-defined fallback in release (use the first occurrence, log via `tracing::warn!` ONLY in release as a diagnostic since the debug assert already gates dev workflows). This is reconciled with the existing `element_tree.rs:522` "GlobalKey hash collision" warn-and-overwrite path — the existing behavior changes to `debug_assert!` + release-fallback symmetrically (see FR-024).
- **`GlobalKey` reparenting.** A keyed child mounted under P1 can move to P2 in the next rebuild. The reparenting flows through the new keyed reconciler — the existing `global_key_registry` becomes an *index* the reconciler consults, not a side-channel that bypasses it (see FR-029).
- **A `View` whose type changes between rebuilds.** Element identity is broken (old element unmounted with full lifecycle, new element mounted). Keys do NOT cross types (`Widget.canUpdate` semantics).
- **Empty child list to non-empty rebuild, and vice versa.** Atomic mount/unmount — no intermediate "partial list" state observable by `paint`.
- **Very large lists (10,000+ children).** Algorithm is O(N) over the list size regardless of permutation pattern (per Flutter's algorithm shape, not O(shift-distance)). Verified by `criterion` benchmark.
- **Static heterogeneous children with N > 16.** Tuple `ViewSeq` impls are macro-generated for arities `0..=16` (matching Rust stdlib's standard tuple-trait cap). A static `Wrap` of 20 filter chips, a `Toolbar` of 25 items, or any other statically-known heterogeneous list with > 16 entries falls back to the dynamic `Vec<BoxedView>` path: the author writes `vec![item1.boxed(), item2.boxed(), ...]`. The fallback is one explicit `.boxed()` per child — visible cost the author can see and reason about. The cliff at 16 is documented; raising it (to 32/64) is a build-time-cost vs. authoring-ergonomics trade-off for a follow-up change, not in scope here.
- **Conditional `build()` return** (`if x { Text(...) } else { Padding {...} }`). The two arms have different types; `impl IntoView` cannot bridge them directly. The author writes one of: (a) `if x { Text(...).boxed() } else { Padding {...}.boxed() }` returning `BoxedView`, or (b) the proposed `view_match!` helper macro `view_match!(x => Text(...), _ => Padding {...})`. Pattern (a) is the canonical fallback; pattern (b) is a small ergonomics helper that may land with C3 if benchmarks show pattern (a) is hit often in the catalog.
- **Recursive widgets** (`TreeNode` returning `TreeNode`-typed children). `impl IntoView` from a recursive `build()` produces an unboundedly-deep `impl Trait`. The fix is `Box<TreeNode>` or `.boxed()` at the recursion edge — same shape Flutter uses (`List<Widget>` is the boundary). Documented in C3 design; flagged here for completeness.
- **A `View` value whose `build()` returns the same `IntoView` expression but a different value across rebuilds.** Framework re-uses the element (type matches), updates the widget reference; `View::can_update` controls whether `build()` is invoked or skipped — see Assumptions on `can_update`.
- **Empty `column!`/`row!` macro** (`column![]`). Produces the unit `()` tuple, which implements `ViewSeq` as zero children.

---

## Requirements *(mandatory)*

### Functional Requirements

All requirements are verifiable by either a compilation result or an integration test. Internal-ordering constraints between FRs are stated inline.

**The `View` trait surface (C4):**

- **FR-001**: The `View` trait MUST be object-safe (the dynamic-children fallback of C2 needs `Box<dyn View>` storage). `View::key()` returning `Option<&dyn ViewKey>` is the existing object-safe form; it stays.
- **FR-002**: The `View` trait MUST NOT carry a lifetime parameter on its public surface. **Rationale**: a lifetime parameter would force every widget author to write `impl<'a> View for MyWidget<'a>` for any widget storing borrowed data, raising the conceptual cost above Flutter; would block storing `View` values in `'static` element-arena slots, requiring boxing or self-referential structs; and would destroy `impl Trait` inference in `build() -> impl IntoView` when the return captures `&self`. The contract is on the *public surface*; the internal representation may evolve (from today's `Arc<RwLock<ElementTree>>` toward the GPUI lease pattern) without breaking widgets.
- **FR-003**: The `View` trait MUST stay as it is today (`create_element` + `view_type_id` + `can_update(&dyn View)` + `key()`). It does NOT gain a `build()` method — `build()` lives on the typed authoring sub-traits per FR-007/FR-008.
- **FR-004**: The `View` trait MUST NOT carry `async fn` on any method invoked from the build / layout / paint hot path (`docs/PORT.md` refusal trigger #3).
- **FR-005**: The arity type-system (`Leaf` / `Single` / `Optional` / `Variable` ZST markers, `RenderBox<A: Arity>`) MUST stay untouched. Render-object widgets continue to parameterize over `Arity`.
- **FR-006**: `Downcast` / `DynClone` bounds MAY remain on `View` to support the `BoxedView` dynamic-children path; they MUST NOT propagate to widget-author code via additional bounds on user types.

**Widget-authoring API (C3):**

- **FR-007**: `StatelessView::build(&self, ctx: &dyn BuildContext) -> impl IntoView` MUST be the new typed return form, replacing today's `Box<dyn View>` return. The `IntoView` trait already exists at `crates/flui-view/src/view/into_view.rs` and is used here. `StatelessView` becomes non-object-safe via the return-position-impl-trait — this is acceptable because no `dyn StatelessView` use exists or is needed (`View` is the object-safe boundary; `StatelessView` is implementation-side).
- **FR-008**: `ViewState::build(&self, view: &Self::View, ctx: &dyn BuildContext) -> impl IntoView` MUST adopt the same `impl IntoView` return. Symmetrical with FR-007.
- **FR-009**: A `#[derive(StatelessView)]` proc-macro MUST exist and generate the canonical `impl View for T` block (the `create_element` + `view_type_id` + `key` boilerplate) so a trivial widget reduces to `#[derive(Clone, StatelessView)] struct T { ... } impl T { fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView { ... } }`. A parallel `#[derive(StatefulView)]` MUST exist for the stateful case. A **new crate `crates/flui-macros`** MUST be created (`proc-macro = true`; dependencies `syn` 2.x, `quote` 1.x, `proc-macro2` 1.x — already in the workspace lockfile via `bon-macros` and `ambassador`, so no new transitive surface) and added to `[workspace.members]` in the same change. `flui-macros` is a leaf crate, positioned upstream of `flui-view` in the workspace DAG. `flui-view` re-exports the derives via `pub use flui_macros::{StatelessView, StatefulView};` in its prelude so widget authors write only `#[derive(StatelessView)]` without an extra `use` line.
- **FR-010**: The existing `impl_stateless_view!` declarative macro MUST be deleted in the same change once `#[derive(StatelessView)]` covers its cases. There MUST NOT be two parallel authoring paths.
- **FR-011**: `bon` builders SHOULD be the recommended pattern for widgets with >3 constructor fields. Whether this is enforced by lint or only by convention is a plan-phase decision; FR-011 commits only that `bon` is the chosen builder dependency and worked examples ship in `flui-widgets`.

**Heterogeneous children (C2):**

- **FR-012**: A `ViewSeq` trait MUST exist in `crates/flui-view/` with at minimum the methods needed to enumerate, type-erase per-position when needed, and report child count. The exact method set is a plan-phase deliverable; the trait's existence and its role as the multi-child boundary is locked here.
- **FR-013**: `ViewSeq` MUST have a macro-generated `impl ViewSeq for (A, B, …, P)` for tuple arities `0..=16`. Tuples of `View`-implementing types are accepted as multi-child widgets' `children` field directly.
- **FR-014**: A `column! { … }` / `row! { … }` declarative macro MUST exist in `flui-view::macros`, expanding to the tuple (`flui-widgets` is not in `[workspace.members]` today, and US2/US3 acceptance scenarios must be testable inside this merge unit — only `flui-view` is feasible). The literal call site `column![Text(…), Button {…}, Image {…}]` is the canonical authoring form. When `flui-widgets` is re-enabled, the macros MAY be re-homed via re-export; the source stays in `flui-view`.
- **FR-015**: A blanket `impl<V: View> ViewSeq for Vec<V>` MUST exist for the homogeneous-dynamic case. A `impl ViewSeq for Vec<BoxedView>` MUST exist for the heterogeneous-dynamic case — **the canonical path every scrolling and data-display widget sits on** (`ListView`, `GridView`, `CustomScrollView`, `DataTable`, every `Vec`/iterator-driven widget, plus static-heterogeneous widgets with > 16 children per Edge Cases). Empirically the dynamic path is NOT the literal "primary path for the catalog" by widget count (Material has ~100× more single-child `child:` slots than `List<Widget> children` lists), but it IS the path the highest-visibility scrollable widgets sit on — the widgets users see and judge the framework by. The design and test corpus treat the dynamic path as equally load-bearing to the tuple path; the SC-002 keyed-reorder test corpus exercises both paths against the same algorithm.
- **FR-016**: The dynamic-path reconciler signature MUST work identically with the tuple path for keyed reordering — `Vec<BoxedView>` children with `Key`s reorder with state preservation (US3 acceptance scenario 1). The performance gap between the two paths is bounded: dynamic path pays `dyn`-dispatch per child; tuple path is monomorphic per position. Both paths share the same algorithm.
- **FR-017**: The existing `Children` / `BoxedView` types MUST be retained, audited, and folded into the `Vec<BoxedView>` path. The current builder-only `Children` API (`.child(x).child(y)`) is deprecated in favor of `column!`/`row!` macros + `Vec<BoxedView>` literals; it MAY remain as a transition path for one release.
- **FR-018**: Multi-child widgets MUST be generic over `C: ViewSeq` (`struct Column<C: ViewSeq> { children: C }`), not specialized over `Vec<BoxedView>` only. This is what enables the tuple-static-path benefits to actually land.

**Element storage (C4 — addresses 6-variant + arity + AnimationBehavior findings):**

- **FR-019**: Element storage MUST be a closed enum `ElementKind` (`#[non_exhaustive]`-managed) used as the inner `kind: ElementKind` field of the existing `ElementNode` struct in `element_tree.rs`. **The outer `struct ElementNode` keeps its current name + its tree-traversal metadata fields** (parent, depth, slot, registered_global_key_hash + new `key` field per FR-022); the change is to replace the `element: Box<dyn ElementBase>` field with `kind: ElementKind`. The name `ElementNode` (struct) and `ElementKind` (enum) MUST be distinct — no name collision.
- **FR-020**: `ElementKind` variants reflect the *real* behavior taxonomy as it exists in `crates/flui-view/UNIFIED_ELEMENT.md`. The variants are: `Stateless(StatelessElementData)`, `Stateful(StatefulElementData)` (where `StatefulElementData` carries `animation_listener: Option<AnimationListener>` — see below), `Proxy(ProxyElementData)`, `Inherited(InheritedElementData)`, and the **Render family as four separate variants** `RenderLeaf(Box<dyn RenderElementBase<Leaf>>)`, `RenderSingle(Box<dyn RenderElementBase<Single>>)`, `RenderOptional(Box<dyn RenderElementBase<Optional>>)`, `RenderVariable(Box<dyn RenderElementBase<Variable>>)`. **Variant choice committed (round-2 doc-review):** four separate `Render*` variants — NOT a single `Render` with an inner arity enum. Rationale: an enum variant cannot introduce a generic parameter the outer enum does not carry, so the inner `ElementCore<V, A>` must be boxed regardless; four variants put the arity discriminant at the *outer* `match` site (where the reconciler dispatches per child), keeping arity-class dispatch monomorphic; one variant with inner arity enum would force a nested `match` inside the inner `RenderElementData`, defeating the per-position monomorphism SC-007 measures. Inner boxing per variant is sanctioned by FR-029 point 1. **`AnimationBehavior` fold mechanism:** at `create_element` time for an `AnimatedView V`, capture a `Box<dyn Fn(&dyn StatefulElementBase) -> Arc<dyn Listenable> + Send + Sync>` closure that obtains the listenable through the typed `V` boundary; the closure is stored alongside the `ListenerId` as `AnimationListener { listenable_factory: Box<...>, listener_id: ListenerId }`. This preserves the `V: AnimatedView` typed dispatch (`view.listenable()`) without making the enum variant generic over `V`. The closure is set once at mount and released on dispose.
- **FR-021**: The runtime `downcast_ref::<V>()` path in element updates (`crates/flui-view/src/element/generic.rs:271`, which today logs `tracing::warn!` on failure) MUST be eliminated. Update dispatch is a typed `match` on `ElementKind`. **FR-021 has an internal-ordering precondition: FR-019 (the `ElementKind` enum) must land first.** The two cannot land atomically split — they land together or FR-021's elimination is impossible.
- **FR-022**: Every `ElementNode` MUST carry a `key: Option<Box<dyn ViewKey>>` field, populated at insertion from `View::key()` and copied at every mount. The existing `registered_global_key_hash` is reduced to a side-index from `key` (lookup, not storage); the new `key` field is authoritative. The `Key` family per `crates/flui-foundation/src/key.rs` and `crates/flui-view/src/key/` (5 key types — `Key` newtype, `ValueKey<T>`, `UniqueKey`, plus `ObjectKey` and `GlobalKey<T>` in flui-view) all MUST be storable; the type discriminant is preserved via `&dyn ViewKey`.
- **FR-023**: Element storage internals — the `Slab` arena, `NonZeroUsize` ID offset, `AtomicRenderFlags`, `PipelineOwnerHandle` — MUST NOT be modified. The change is the *shape* of each slab entry's stored data; the arena machinery does not change.

**Keyed reconciliation (C6 — addresses "stub not starting point" finding):**

- **FR-024**: Variable-arity child reconciliation MUST execute Flutter's keyed linear O(N) algorithm. **Honest accounting**: the existing `crates/flui-view/src/tree/reconciliation.rs::reconcile_children` is a *scaffold* — its start/end fast paths work, its middle/keyed section is a TODO stub (`reconciliation.rs:91-98`: "we don't have direct access to the original View's key … This would need enhancement to store keys in ElementNode"), and its tests cover zero keyed cases. FR-024 work is therefore: (a) complete the keyed middle section once FR-022 has stored keys; (b) write the keyed-reorder test corpus (every permutation of `[A(k1), B(k2), C(k3)]` per SC-002 + the GlobalKey reparenting tests + the hash-collision tests); (c) replace `view_type_id`-based old-side keying with `ViewKey::key_hash` + `ViewKey::key_eq` (real equality on hash hit). The "starting point" framing in the round-1 spec was wrong; this requirement honestly states the work.
- **FR-025**: The current positional `VariableChildStorage::update_with_views` at `crates/flui-view/src/element/child_storage.rs:494` MUST be **deleted** in the same change, AND `ElementCore<V, Variable>::update_or_create_children` MUST be rewired to invoke `tree::reconcile_children` directly (today's production hot path is `ElementCore::update_or_create_children → ElementChildStorage::update_with_views → VariableChildStorage::update_with_views`; deletion without rewiring leaves Variable child updates silently inert). The rewiring MAY require splitting `ElementChildStorage` between `Single`/`Optional` arities (which keep `update_with_view`) and `Variable` (which routes through `tree::reconcile_children`). There MUST NOT be two implementations of variable-arity reconciliation in the workspace.
- **FR-026**: On a keyed reorder, element identity MUST be preserved — `Element::id()` remains the same value, `State<W>` objects are not re-instantiated, lifecycle hooks (`init_state`, `dispose`) are not re-invoked.
- **FR-027**: On a type mismatch at the same position, element identity MUST NOT be preserved. Old element unmounted with full lifecycle, new element mounted. Keys do NOT match across types.
- **FR-028**: `View::can_update` semantics MUST be: `runtimeType == other.runtimeType && key == other.key` (matching Flutter's `Widget.canUpdate`). Today only the first half holds (`view_type_id() == other.view_type_id()`). FR-028's expansion follows FR-022 storing the key.

**Type-erasure boundary (C9 corollary):**

- **FR-029**: New `dyn`-trait-object boundaries MUST NOT be introduced beyond the **three** sanctioned points: (1) the **element storage** enum of FR-019 (sanctioned because the enum closes the set), (2) the **dynamic-children fallback** `Vec<BoxedView>` of FR-015 (opt-in, primary path for dynamic widgets), (3) the **platform backend** `Box<dyn PlatformWindow>` and equivalent platform-trait boundaries (selected once at startup, genuinely open, off the hot path — explicitly sanctioned in `docs/FOUNDATIONS.md` Part III C9). The existing `View::key() -> Option<&dyn ViewKey>` and `&dyn BuildContext` are also pre-existing sanctioned `dyn` surfaces; they stay. The hot path from `StatelessView::build` / `ViewState::build` through to the element insertion remains concrete-typed.
- **FR-030**: `GlobalKey` reparenting MUST flow through the new keyed reconciler — the existing `global_key_registry` (the `register_global_key_view` / `register_global_key_state` / `take_global_key_state` machinery in `element_tree.rs`) becomes an **index** the keyed reconciler consults during the middle-section lookup, not a side-channel that bypasses it. **The integration MUST preserve the registry's O(1) cross-tree lookup** — for any `GlobalKey<W>(K)`, `owner.element_for_global_key(K)` returns the current `ElementId` regardless of which parent owns it. The reconciler consults this lookup on each unmatched-by-position keyed child (cost is O(unmatched-keyed-children) per parent rebuild, NOT O(tree-size)). Cross-parent reparenting (`GlobalKey<W>(K)` mounted under P1 moves to P2 in P2's rebuild) detaches the element from P1's child list lazily before the next layout pass. Failing to wire it means SC-003 passes via the existing side-channel without exercising the new reconciler (false-pass per the doc-review finding).

**Migration & compatibility:**

- **FR-031**: All blanket `impl View for ...` and `impl StatelessView for ...` / `impl ViewState for ...` blocks affected by FR-007/FR-008 (the `build() -> Box<dyn View>` → `build() -> impl IntoView` change) MUST be updated in the same change set. The enumeration:
  - **Canonical trait declarations** — `crates/flui-view/src/view/stateless.rs:49` (`StatelessView::build` signature) and `crates/flui-view/src/view/stateful.rs:116` (`ViewState::build` signature). These are the primary migration targets; without them, every `impl` block produces a build error.
  - **Production impl sites** — every `impl StatelessView for ...` and `impl ViewState for ...` in `crates/flui-view/src/view/` (`stateless.rs`, `stateful.rs`, `inherited.rs`, `proxy.rs`, `render.rs`, `animated.rs`).
  - **Test impl sites** — every `impl StatelessView for ...` (~24 occurrences) and `impl ViewState for ...` across `crates/flui-view/tests/` (notably `view_element_conversion_tests.rs`, `lifecycle_tests.rs`, `ancestor_finders.rs`, `global_key.rs`).
  - **App-level glue** — `crates/flui-app/src/app/runner.rs:628` (test scaffolding `impl StatelessView` block).
  - **Orphan deletion** — `crates/flui-view/src/wrappers/render.rs` MUST be **deleted** (not re-architected). It implements a parallel `IntoView` trait (`-> Box<dyn ViewObject>`) incompatible with the canonical surface; its module is not exported from `lib.rs`; no production caller exists. Re-architecting the parallel `ViewObject` / `RenderView<P>` / `ViewMode` surface is out of scope.
  - **Documentation examples** — `crates/flui-hot-reload/src/plugin.rs:121` doc-comment example (`/// impl StatelessView for MyApp { fn build(...) -> Box<dyn View> { ... } }`) MUST be updated to the new return type, otherwise `cargo test --doc` fails.
  - **Not in scope of this change**: `crates/flui-cli/src/templates/` (template emitters). The current templates reference unbuilt crates (`flui-widgets`, `MaterialApp`, `Scaffold`) and are fictional placeholder code; rewriting them is deferred to Catalog.1 along with re-enabling `flui-cli` in `[workspace.members]`. See Out of Scope.
- **FR-032**: The change set MUST leave `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` green.
- **FR-033**: A static-analysis grep MUST be added to `scripts/port-check.sh` scoped to the **View-type update dispatch path specifically** — the body of `ElementCore::update_view` at `crates/flui-view/src/element/generic.rs:271` and any function it transitively calls during view-update. Legitimate non-update-path uses of `downcast_ref` MUST be whitelisted explicitly: `crates/flui-view/src/element/unified.rs::insert_render_object_child` (`unified.rs:313` — casts `&dyn Any → &RenderId` for slot attachment, not View type dispatch), `crates/flui-view/src/element/unified.rs::remove_render_object_child` (`unified.rs:351` — symmetrical), and any future similar slot-management code that operates on `&dyn Any` rather than `&dyn View`. The whitelist is maintained as a `// PORT-CHECK-OK: <reason>` line-comment marker that `port-check.sh` parses. The grep is added independently of refusal trigger #8 — trigger #8 detects stubbed-but-called methods, which is a different defect class.

### Key Entities *(framework concepts surfaced by this contract)*

- **`View`** — immutable widget configuration trait widget authors implement (mostly via `#[derive]`). Object-safe, no public lifetime parameter, no `build()` method on the trait itself.
- **`StatelessView` / `StatefulView` / `ViewState`** — typed authoring traits where `build()` lives. `build() -> impl IntoView`. Non-object-safe (acceptable — no `dyn StatelessView` use).
- **`IntoView`** — conversion trait `build()` returns. Lets the framework accept any concrete `View` value without the author writing boxing syntax.
- **`ViewSeq`** — heterogeneous-children trait. Implemented by tuples `(A, B, ..., P)` for arities `0..=16` (static path) and by `Vec<BoxedView>` (dynamic path, primary for the scrolling/data-display catalog).
- **`BoxedView`** — opt-in `Box<dyn View>` wrapper for the dynamic-children path. The `.boxed()` extension method on `View` produces it.
- **`Key` family** — optional identity marker on a `View` / `ElementNode`. Five concrete types: `Key` (NonZeroU64 newtype), `ValueKey<T>`, `UniqueKey` (in `flui-foundation`), `ObjectKey` and `GlobalKey<T>` (in `flui-view`). All implement `trait ViewKey`. Drives keyed reconciliation.
- **`ElementNode`** — the existing struct in `element_tree.rs` wrapping a tree node. Carries `parent`, `depth`, `slot`, `key`, plus the new `kind: ElementKind` field replacing today's `element: Box<dyn ElementBase>`.
- **`ElementKind`** — closed enum over the behavior set (`Stateless`, `Stateful` (with optional animation-listener field), `Proxy`, `Inherited`, and the `Render` family — see FR-020). `#[non_exhaustive]`.
- **Reconciler** — the keyed O(N) linear algorithm matching new `View` children to existing `ElementNode` children. Lives in `tree/reconciliation.rs`; today's scaffold gets its keyed middle section completed in this change.
- **Element identity** — stable `ElementId` (`NonZeroUsize`) assigned at mount; preserved across compatible rebuilds (type match + key match); broken on type mismatch.

---

## Success Criteria *(mandatory)*

Every criterion is either a passing test, a `cargo` exit code, or an objective measurement. Criteria that defer validation to a later phase are explicitly marked.

- **SC-001**: A widget author writes the trivial stateless `Greeting { name: String }` widget in **≤ 7 lines** of `rustfmt`-formatted widget-author source (idiomatic Flutter parity: dartfmt'd `class Greeting extends StatelessWidget { final String name; const Greeting({super.key, required this.name}); @override Widget build(BuildContext c) => Text(name); }` is 6-7 lines depending on `@override` placement; rustfmt forces opening braces on own lines for `impl` blocks, so the FLUI minimum is 7). The FLUI source contains **no** `Box::new`, **no** explicit `impl View for Greeting` block, **no** `impl_stateless_view!` invocation, **no** `.into_view()` call. The SC is measured against `rustfmt`-canonical output, not source-as-typed.
- **SC-002**: The integration-test suite for keyed reconciliation passes **100%** of cases: a list `[A(key=k1), B(key=k2), C(key=k3)]` reordered to each of the 6 permutations preserves the per-item counter state of every item. Test asserts both per-item state preservation AND `ElementId` identity preservation. Exercised against **both** the tuple-static `ViewSeq` path AND the dynamic `Vec<BoxedView>` path.
- **SC-003**: `GlobalKey` reparenting test passes — a keyed child mounted under one parent moves to a different parent in the next rebuild without state loss and without `State::dispose` invocation. The test asserts the reparenting flows **through** the new keyed reconciler (the test instruments the reconciler entry and asserts it is called); a pass via the existing `global_key_registry` side-channel does NOT satisfy SC-003.
- **SC-004**: The runtime `downcast_ref::<V>()` call count in the **View-type update dispatch path** (the body of `ElementCore::update_view` at `crates/flui-view/src/element/generic.rs:271` and its transitive call graph during view-update) is **0** — verified by the FR-033 grep. Legitimate non-View-type `downcast_ref` uses (`unified.rs:313/351` for slot attachment, etc.) are explicitly whitelisted via `// PORT-CHECK-OK:` markers and do NOT fail this SC.
- **SC-005**: `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` exit 0 on the merge commit. `bash scripts/port-check.sh -v` exits 0 with all 7 existing triggers green; the new `downcast_ref`-in-update-path grep is green.
- **SC-006**: A `criterion` benchmark exercising keyed reordering on N=10,000 children executes in time **linear in N regardless of permutation pattern** (full-reverse, single-rotate, swap-first-and-last all stay within a constant factor of N). Not O(shift-distance), which is a different and smaller bound the round-1 spec confused with O(N).
- **SC-007**: The trivial `column!` static-tuple authoring path compiles without `Box::new` or `.into_view()` in the author's source. The reconciler's outer `match self.kind { … }` dispatch (per FR-020's four-Render-variant choice) is **monomorphic per arity class** — the arity discriminant is visible at the outer match site, eliminating arity-level dispatch overhead. Each child still pays one bounded `dyn`-call to its arity-specific `RenderElementBase<A>` impl (sanctioned per FR-029 point 1); this is a per-child cost, not a per-position cost, and is the same overhead regardless of static-tuple vs dynamic-Vec authoring. Verified by `cargo-asm` or equivalent showing no nested-match (arity-within-Render) instruction in the reconciler inner loop.
- **SC-008**: The trivial dynamic-children `Vec<BoxedView>` path (US3) renders a 1,000-item list of heterogeneous keyed children, supports US1's reorder scenario (state preserved on permutation), and passes the same SC-002 keyed-reorder test corpus.
- **SC-009**: A conditional `build()` return using `BoxedView` (`if x { Text("a").boxed() } else { Padding {...}.boxed() }`) compiles and runs. The author-side overhead vs the trivial `impl IntoView` return is **≤ 2 additional tokens per branch** (`.boxed()`). This makes the impl-Trait ergonomic cliff measurable, not unstated.
- **SC-010**: An initial subset of Flutter's `framework_test.dart` keyed-reconciliation cases — **at minimum**: every test in `.flutter/flutter-master/packages/flutter/test/foundation/key_test.dart` and the keyed-reconciliation tests in `.flutter/flutter-master/packages/flutter/test/widgets/key_test.dart` — is ported behavior-faithfully and passes against the FLUI implementation. This is the start of the parity oracle infrastructure ROADMAP "What parity means" defines.
- **SC-011**: A framework contributor adds an unused variant to `ElementKind` and `cargo build` reports every non-exhaustive `match` site that needs updating. Verified by a CI smoke test that builds against a feature-flagged stub variant.
- **SC-012**: GlobalKey reparenting performance bound — a `GlobalKey<W>(K)`-bearing subtree moved from parent P1 to parent P2 during P2's rebuild completes in **O(subtree depth + 1)**, NOT O(tree size). Verified by a `criterion` benchmark with a tree of 10,000 elements containing a 10-node subtree under `GlobalKey<W>(K)` reparented across parents on each iteration.

---

## Implementation Sequence (rollback granularity)

Per round-2 doc-review (adversarial ADV-R2-05): the unification of C2+C3+C4+C6 into one spec does NOT mean the implementation lands in a single atomic commit. The contracts are **designed together** (this spec is the single source of truth); implementation lands in **three sequenced PRs** so a defect in a later phase does not roll back earlier work:

1. **Phase 1 — Storage shape + key field** (FR-019, FR-020, FR-022, FR-023). Land `ElementKind` enum (four `Render*` variants), the new `key: Option<Box<dyn ViewKey>>` field on `ElementNode`, and the `flui-macros` crate skeleton (FR-009). The runtime `downcast_ref::<V>()` in `generic.rs:271` is retained behind a `#[deprecated]` marker to keep workspace green; FR-021's elimination is deferred to Phase 3.
2. **Phase 2 — Keyed reconciler completion + `ElementCore` rewiring** (FR-024, FR-025, FR-026, FR-027, FR-028, FR-030). Complete the keyed middle section of `reconcile_children`; wire `ElementCore<V, Variable>::update_or_create_children` to it; delete the positional `update_with_views`; wire the `global_key_registry` index. Phase 2 lands on the Phase-1 foundation.
3. **Phase 3 — `IntoView` surface + `downcast_ref` elimination + derive macros + `port-check.sh` grep** (FR-007, FR-008, FR-009 macros, FR-010, FR-021, FR-031, FR-033). The widget-author-facing change set. Phase 3 lands on the Phase-2 foundation, which lands on Phase-1.

Each phase keeps `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` green; a defect surfaced in Phase 2 reverts only Phase 2's PR, leaving Phase 1 intact. The contract is locked atomically (this spec); the merge is staged. This trades unified-merge simplicity for revert granularity — the round-2 finding that the unified merge had no rollback plan.

---

## Assumptions

- **Reconciler scaffold honestly framed.** The existing `crates/flui-view/src/tree/reconciliation.rs::reconcile_children` is a scaffold whose start/end fast paths work but whose keyed middle section is a stub (verified at `reconciliation.rs:91-98` and by the absence of keyed-reorder test cases in `tests/reconciliation_tests.rs`). FR-024 completes the algorithm; it does not "wire up" a complete implementation.
- **Algorithm source.** The keyed linear algorithm is ported from `.flutter/flutter-master/packages/flutter/lib/src/widgets/framework.dart` `RenderObjectElement.updateChildren`. The algorithm's correctness is established by SC-010 (the ported test corpus), not by the current state of `reconcile_children`.
- **Element-behavior taxonomy.** Six top-level concepts (Stateless, Stateful with optional animation listener, Proxy, Inherited, plus the Render family). The Render family is one of four-arity-variants or one-variant-with-inner-arity-enum — plan-phase decision. The naming choice is `ElementKind` for the enum, `ElementNode` retained as the outer struct wrapping `kind: ElementKind` + tree metadata; no name collision.
- **`AnimationBehavior` composition.** `AnimationBehavior` today composes `StatefulBehavior` (not a peer). The closed enum reflects this by folding animation into `Stateful` as an optional `animation_listener` field, not by exposing a peer `Animation` variant.
- **`Key` taxonomy.** Five concrete `ViewKey` impls (`Key`, `ValueKey<T>`, `UniqueKey` in `flui-foundation`; `ObjectKey`, `GlobalKey<T>` in `flui-view`). The field on `ElementNode` is `Option<Box<dyn ViewKey>>` (using the existing `ViewKey` trait, not the `Key` newtype).
- **`can_update` form.** The current object-safe `fn can_update(&self, old: &dyn View) -> bool` is the permanent form for FR-001 (object-safety). Per-`Self` typed `can_update` (FOUNDATIONS Part II item 4) is a separate optimization that may layer on top via a non-object-safe extension trait (the `Memo<V>` combinator) — not in scope of this contract.
- **`BuildContext` shape.** Contract C5 (`BuildContext` `new_minimal` correctness hole) is a separate work item, tracked in Cross.H (gates Catalog.1, not this spec). This spec assumes the existing object-safe `BuildContext` trait shape and does not modify it. The `new_minimal` hole does not block this contract from landing because the contract's own tests do not depend on `Theme`/`InheritedView` lookup.
- **`BoxedView` already exists.** `BoxedView` is defined at `crates/flui-view/src/view/into_view.rs:142` and is the canonical wrapper for the dynamic path. C2's FR-015 makes it first-class, not a fallback.
- **Breaking changes authorized.** Per `STRATEGY.md` and explicit user direction, the migration may break any internal API necessary. All affected sites in `crates/flui-view/`, `crates/flui-app/`, `crates/flui-hot-reload/`, `crates/flui-cli/templates/`, and consumers are updated in the same change set per FR-031.
- **No new refusal trigger #8 needed for SC-004.** SC-004 is enforced by a dedicated grep in `port-check.sh` (FR-033), not by refusal trigger #8 (which detects stubbed-but-called methods — a different defect class). The round-1 spec's attribution of SC-004 to trigger #8 was wrong.

---

## Dependencies

- **`docs/FOUNDATIONS.md`** Part III contracts C2, C3, C4, C6, C9 (canonical text); Part II item 4 (`memoize` cross-link).
- **`docs/ROADMAP.md`** Core.0 phase; the four-layer model.
- **`docs/research/2026-05-22-architectural-contracts.md`** Contracts 2 (View trait / `dyn` boundary), 3 (heterogeneous children), 5 (reconciliation), 6 (widget-authoring API).
- **`docs/research/2026-05-22-technology-adoption-matrix.md`** Subsystems 1 (three trees + ownership), 2 (reconciliation), 11 (reactivity / `setState`), 13 (type-erasure boundary).
- **`docs/research/2026-05-22-architecture-correction-plan.md`** D-2 (the index-vs-key defect — addressed by FR-024+FR-025), SP-2 (written-but-uncalled pattern this contract closes).
- **`docs/research/2026-05-22-rust-ui-ecosystem-lessons.md`** Xilem `ViewSequence` (the source pattern for FR-012/FR-013), GPUI element-handle persistence (lifecycle cross-reference).
- **`.flutter/flutter-master/packages/flutter/lib/src/widgets/framework.dart`** — `Element.updateChild`, `RenderObjectElement.updateChildren`, `Widget.canUpdate` (the algorithms and semantics).
- **`crates/flui-view/UNIFIED_ELEMENT.md`** — the existing element-behavior taxonomy `ElementKind` mirrors.
- **`crates/flui-view/src/view/`** — `View`, `IntoView`, `BoxedView`, `StatelessView`, `ViewState`, `StatefulView` (existing trait surfaces).
- **`crates/flui-view/src/tree/reconciliation.rs`** — the scaffold whose keyed middle section is completed in this change.
- **`crates/flui-view/src/element/child_storage.rs`** — the positional `update_with_views` (line 494) this change deletes.
- **`crates/flui-view/src/element/generic.rs:271`** — the `downcast_ref::<V>()` site FR-021 eliminates.
- **`crates/flui-foundation/src/key.rs`** + **`crates/flui-view/src/key/`** — the `ViewKey` trait and 5-concrete-type taxonomy.
- **`crates/flui-cli/src/templates/`** — template emitters that produce user-facing code; must be updated.

---

## Out of Scope

The following contracts are explicitly NOT in this change set:

- **C1 — Reactivity / state model.** `setState` + `InheritedWidget` + signals-out — ratified in FOUNDATIONS Part III; the implementation work is independent of this contract.
- **C5 — `BuildContext` `new_minimal` correctness hole.** Cross.H repair (gates Catalog.1, not Core.1). Independent of this contract.
- **C7 — `build()` error model + `catch_unwind` boundary.** Ratified in FOUNDATIONS Part III; the framework-level `catch_unwind` placement is its own follow-up.
- **C8 — async edges.** Enforced by PORT.md refusal trigger #3, no new work needed.
- **The typed `View::can_update` / `Memo<V>` combinator** (FOUNDATIONS Part II item 4). The object-safe `can_update(&dyn View)` form is the permanent View-trait method; a typed `Memo<V>` combinator is a separate post-contract optimization.
- **The D-1 / D-3 / D-4 stubbed render phases.** Separate Core.0 work items, not blocked by this contract.
- **The `flui-app::theme::colors` parallel `Color` deletion** (Cycle 5 V-25). Tracked separately under Cross.H.
- **Widget catalog widgets.** Nothing in `flui-widgets` is built in this change. The change produces the contract surface; the catalog itself is Business.1.
- **Refusal triggers #8-#13.** They are written into `PORT.md` as a separate Core.0 work item; this change adds **one** `downcast_ref`-in-update-path grep to `port-check.sh` per FR-033, independent of #8-#13.
- **`flui-cli` re-enable + Material-shaped templates.** Per round-2 doc-review: the current `crates/flui-cli/src/templates/basic.rs` and `counter.rs` reference unbuilt crates (`flui_widgets::*`, `MaterialApp`, `Scaffold`, `AppBar`, `FloatingActionButton`, `use_signal`, `ThemeData::light()`) and are fictional placeholder code. `flui-cli` is commented out of `[workspace.members]` and the CI cannot run the round-1 SC-012 (`cargo run --bin flui-cli new test_app && cargo build`) against current code. Both the `flui-cli` re-enable AND the template rewrite to use the new contract surface DEFER to **Catalog.1** when the referenced widgets actually exist. This contract change does NOT touch `crates/flui-cli/src/templates/` and the original SC-012 is replaced by the GlobalKey-reparenting performance bound.

---

## Deferred / Open Questions

Items the doc-review surfaced that the plan phase will resolve:

- ~~**Render-variant shape choice** (FR-020 part).~~ **Resolved round-2:** FR-020 commits to four separate `Render*` variants. Rationale in FR-020 text.
- ~~**`column!` / `row!` macro location.**~~ **Resolved round-2:** FR-014 commits the macros to `flui-view::macros` (the only feasible home today; `flui-widgets` is not in `[workspace.members]`).
- **`view_match!` helper macro for conditional returns.** Pattern (b) in the Edge Cases conditional-return discussion. If benchmarks show pattern (a) (`.boxed()` per branch) is hit often enough in `flui-material`, ship the helper macro with C3; otherwise defer to a future ergonomics pass.
- **`bon` enforcement.** Lint or convention only — FR-011 commits to `bon` as the chosen builder dep without fixing the enforcement mechanism.
- **Memory cost of `Option<Box<dyn ViewKey>>` per ElementNode.** A 16-byte `Option<Box>` tail on every node, including the unkeyed-leaf majority. For a 10,000-element list, ~160KB of unused storage. Plan-phase: measure and decide whether to use an `Option<KeyId>` interned-key approach instead (trades a hash lookup per access for storage savings).

---
