# Tree — Surface Reduction Specification

## Purpose

Pin the canonical disposition for every `flui-tree` module / type
that cycle 3 deleted, kept, or kept-with-caveats. Pre-cycle-3 the
crate carried ~10,600 LOC of zero-consumer scaffolding (visitor/,
diff.rs, cursor/path/breadth_first/depth_first iterators, the
arity-storage machinery, the `state.rs` typestate, the `Node`
trait, `MountableExt`). Cycle 3 chose **delete** over
**feature-gate** per the `no-quick-wins-vanyastaff` memory rule
("feature-gated dead code is still maintenance burden + still
CI-compile overhead"). This spec records each deletion as
**permanent** with the revival trigger spelled out, so a future
devtools workstream knows to port from git history rather than
re-inventing the surface.

The audit's recommended dispositions (T-3 through T-8) prescribed
feature-gating; cycle 3's `no-quick-wins-vanyastaff` decision
chose deletion. This spec records cycle 3's stronger choice as
canonical and notes the audit-recommended fallback (`unstable-devtools`
feature-gate) as the revival shape if any module is restored.

Owner crate: `crates/flui-tree`.

## Requirements

### Requirement: state.rs (Mountable / Unmountable typestate) is deleted

`crates/flui-tree/src/state.rs` MUST NOT exist. The four typestate
types `Mountable`, `Unmountable`, `Mounted`, `Unmounted`, the
`NodeState` enum, and the `MountableExt` extension trait MUST NOT
exist in the crate's public or private surface.

The crate's `lib.rs` MUST NOT declare `pub mod state;`. The prelude
MUST NOT re-export any of the typestate types.

**Audit ref:** T-3 (closed Wave 1+2 — entire 616-LOC module
deleted; verdict ratified as **permanent**). T-15 partial
(`MountableExt` deleted as a sibling of state.rs deletion).

**Flutter ref:** Flutter's Element FSM (`framework.dart::Element._lifecycleState`)
is **four-state** (`initial` / `active` / `inactive` / `defunct`).
FLUI's `state.rs` was a **two-state** generalisation that did not
match Flutter parity and had zero in-workspace consumers (Element
lifecycle lives in `flui-view/src/element/lifecycle.rs` with its
own four-state FSM).

**Rust-native divergence (vs Flutter):**
- (a) Flutter: Element-only four-state FSM.
- (b) FLUI: per-tree lifecycle. Element lifecycle in `flui-view`;
  Layer lifecycle via `LayerNode::disposed: AtomicBool` + Drop
  (cycle 2 PR #100 U8); Render lifecycle via the RenderObject
  protocol. No generic typestate needed.
- (c) Zero consumers broken — confirmed cycle-3.

**Revival trigger:** A future workspace need materialises for a
generic two-state mount/unmount typestate that is type-checked
across tree boundaries. At that point, port from git history at
the commit predating PR #105 (deletion); place behind a
`unstable-typestate` feature gate; require a real consumer
before re-exposing through the prelude.

#### Scenario: state.rs file does not exist

- GIVEN the repository at HEAD
- WHEN `test -f crates/flui-tree/src/state.rs` is run
- THEN it MUST exit non-zero

#### Scenario: lib.rs does not declare state module

- GIVEN `crates/flui-tree/src/lib.rs`
- WHEN searched for `pub mod state`
- THEN zero matches MUST appear

#### Scenario: Mountable / Unmountable / MountableExt symbols unresolvable

- GIVEN a downstream test crate that depends on `flui-tree`
- WHEN it attempts `use flui_tree::{Mountable, Unmountable, MountableExt};`
- THEN the build MUST fail with unresolved imports

---

### Requirement: visitor/ module (StatefulVisitor / TypedVisitor / TreeVisitor / composition / fallible) is deleted

`crates/flui-tree/src/visitor/` MUST NOT exist. The types
`TreeVisitor`, `TreeVisitorMut`, `TypedVisitor`, `StatefulVisitor`,
`CollectVisitor`, `CountVisitor`, `FindVisitor`, `MaxDepthVisitor`,
`ForEachVisitor`, `ComposedVisitor`, `TripleComposedVisitor`,
`MappedVisitor`, `ConditionalVisitor`, `DynVisitor`, `VisitorVec`,
`VisitorExt`, `FallibleVisitor`, `DepthLimitVisitor`,
`TryCollectVisitor`, `TryForEachVisitor`, `VisitorError`, and the
`states::{Initial, Started, Finished}` typestate marker types MUST
NOT exist.

The crate's `lib.rs` MUST NOT declare `pub mod visitor;`. The
prelude MUST NOT re-export any visitor type.

**Audit ref:** T-4 (closed Wave 4+5 — ~2,560 LOC deleted; verdict
ratified as **permanent**).

**Flutter ref:** Flutter has `Element.visitChildren(visitor:)` etc.
on a per-class basis. FLUI's deleted `TreeVisitor` was a
**generic** visitor over any tree (zero consumers).

**Rust-native divergence:**
- (a) Flutter: per-class visitor methods (`visitChildren`,
  `visitChildrenForSemantics`, etc.) — eight separate APIs.
- (b) FLUI's deletion choice: closure-based iteration via
  `tree.descendants(root).filter(...).collect::<Vec<_>>()` covers
  the visitor pattern's typical use case more ergonomically in
  Rust. The visitor object hierarchy is the classical OO pattern
  Rust closures replace.
- (c) Zero consumers broken — visitor surface had no in-workspace
  consumers.

**Revival trigger:** Future devtools workstream needs structured
tree-walk callbacks with named states / composition. Port from
git history (pre-PR #105). Place behind a `unstable-devtools`
feature gate; require a real consumer (the devtools binding)
before re-exposing through the prelude.

#### Scenario: visitor/ directory does not exist

- GIVEN the repository at HEAD
- WHEN `test -d crates/flui-tree/src/visitor` is run
- THEN it MUST exit non-zero

#### Scenario: visitor types unresolvable

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{TreeVisitor, StatefulVisitor, CollectVisitor};`
- THEN the build MUST fail with unresolved imports

---

### Requirement: diff.rs (TreeDiff / DiffOp / ChildDiff / ChildOp / DiffStats) is deleted

`crates/flui-tree/src/diff.rs` MUST NOT exist. The types `TreeDiff<I>`,
`DiffOp<I>`, `ChildDiff<I>`, `ChildOp<I>`, `DiffStats`, and the
internal `TreeDiffer<'a, I, T>` MUST NOT exist.

The crate's `lib.rs` MUST NOT declare `pub mod diff;`. The prelude
MUST NOT re-export any diff type.

**Audit ref:** T-5 (closed Wave 4+5 — 1,234 LOC deleted; verdict
ratified as **permanent**).

**Flutter ref:** Flutter reconciliation lives in
`widgets/framework.dart::Element.updateChild` (per-class diffing),
NOT in a generic tree-diff module. FLUI's deleted `TreeDiff` was
a speculative generic that flui-view's actual reconciliation
(key-based child reconciliation in `flui-view/src/element/`) did
not use.

**Revival trigger:** Future devtools / hot-reload workstream needs
generic tree-diff for inspector visualisation or scene-plugin
reload. Port from git history; place behind `unstable-devtools`
gate.

#### Scenario: diff.rs file does not exist

- GIVEN the repository at HEAD
- WHEN `test -f crates/flui-tree/src/diff.rs` is run
- THEN it MUST exit non-zero

#### Scenario: TreeDiff / DiffOp symbols unresolvable

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{TreeDiff, DiffOp, ChildDiff};`
- THEN the build MUST fail with unresolved imports

---

### Requirement: iter/cursor.rs, iter/path.rs, iter/breadth_first.rs, iter/depth_first.rs are deleted

The four files `crates/flui-tree/src/iter/cursor.rs`,
`crates/flui-tree/src/iter/path.rs`,
`crates/flui-tree/src/iter/breadth_first.rs`, and
`crates/flui-tree/src/iter/depth_first.rs` MUST NOT exist.

The types `TreeCursor`, `TreePath`, `IndexPath`, `TreeNavPathExt`,
`BreadthFirstIter`, `DepthFirstIter`, `DepthFirstOrder` MUST NOT
exist in the crate's surface.

The four iterator types that remain (`Ancestors`,
`AncestorsWithDepth`, `Descendants`, `DescendantsWithDepth`,
`Siblings`, `AllSiblings`, `SiblingsDirection`, `IndexedSlot`,
`Slot`, `SlotBuilder`, `SlotIter`) MUST stay in
`crates/flui-tree/src/iter/`.

**Audit ref:** T-6 (closed Wave 4+5 — ~3,800 LOC deleted; verdict
ratified as **permanent**). T-25 obsolete (`DepthFirstOrder` enum
deleted with `depth_first.rs`).

**Flutter ref:** Flutter has per-class traversal helpers
(`Element._depthFirstWalkChildren`); no generic
`BreadthFirstIter` / `DepthFirstIter` modules.

**Revival trigger:** Future devtools needs path/cursor primitives
for selection / serialization / inspector. Port from git history;
`unstable-devtools` gate.

#### Scenario: Four deleted iterator files do not exist

- GIVEN the repository at HEAD
- WHEN `test -f crates/flui-tree/src/iter/cursor.rs`,
  `test -f crates/flui-tree/src/iter/path.rs`,
  `test -f crates/flui-tree/src/iter/breadth_first.rs`,
  `test -f crates/flui-tree/src/iter/depth_first.rs`
  are run sequentially
- THEN all four MUST exit non-zero

#### Scenario: TreeCursor / TreePath / IndexPath / BreadthFirstIter / DepthFirstIter unresolvable

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{TreeCursor, TreePath, IndexPath, BreadthFirstIter, DepthFirstIter};`
- THEN the build MUST fail with unresolved imports

#### Scenario: Remaining iterator types are exposed via the prelude

- GIVEN `crates/flui-tree/src/lib.rs`
- WHEN searched for `pub use iter::{` block
- THEN it MUST enumerate `Ancestors`, `Descendants`, `Siblings`,
  `IndexedSlot`, `Slot` at minimum (the load-bearing surviving
  iterators per cycle-3 close)

---

### Requirement: arity/storage.rs, arity/arity_storage.rs, arity/accessors.rs are deleted; arity markers remain

The three files `crates/flui-tree/src/arity/storage.rs`,
`crates/flui-tree/src/arity/arity_storage.rs`, and
`crates/flui-tree/src/arity/accessors.rs` MUST NOT exist.

The types `ChildrenStorage`, `ChildrenStorageExt`,
`ArityStorage<T, A>`, `ChildrenAccess`, `NoChildren`,
`OptionalChild`, `FixedChildren`, `SliceChildren`,
`BoundedChildren`, `SmartChildren`, `TypedChildren`,
`NeverAccessor`, `Copied`, plus the `arity/runtime.rs` and
`arity/aliases.rs` modules MUST NOT exist.

The arity **markers** (`Leaf`, `Single`, `Optional`, `Variable`,
`Exact<N>`, `AtLeast<N>`, `Range<N, M>`, `Never`) MUST remain
in `crates/flui-tree/src/arity/types.rs`. The simplified `Arity`
trait MUST remain. `ArityError` MUST remain.

**Audit ref:** T-7 (closed Wave 4+5 — ~3,000 LOC deleted;
markers + simplified Arity trait kept; verdict ratified as
**permanent**). T-21 / T-22 / T-23 obsolete (types.rs rewritten
in Wave 4+5 — `Leaf::first_impossible` no longer exists,
`#[derive(Default)]` applied to `Never`, arity-storage-tied
associated constants deleted with storage).

**Flutter ref:** Flutter uses `RenderObjectWithChildMixin<ChildType>`
and `ContainerRenderObjectMixin<ChildType, ParentDataType>` for
single-child vs multi-child render objects. FLUI's deleted
`ArityStorage` was a speculative generic enum; the actual
flui-rendering shape uses per-arity-type fields
(`pub struct RenderPadding { child: BoxChild<Single> }`) — a
different concrete pattern that did not need the storage
machinery.

**Rust-native divergence:**
- (a) Flutter: per-class mixin (Dart mixin syntax).
- (b) FLUI: per-arity-type field on the render-object struct;
  arity markers act as type-level binding tags. This is more
  ergonomic in Rust than the deleted `ArityStorage<T, A>` enum.
- (c) Arity markers (`Leaf`, `Single`, `Optional`, `Variable`)
  remain because flui-rendering's render-objects use them as
  type-level binding tags. The deletion is specifically the
  storage layer, NOT the markers.

**Revival trigger:** Future render-object pattern materialises
that wants generic arity-storage as a runtime enum (e.g. for
hot-reload of arity-typed render-trees). Port from git history.

#### Scenario: Three deleted arity storage files do not exist

- GIVEN the repository at HEAD
- WHEN `test -f crates/flui-tree/src/arity/storage.rs`,
  `test -f crates/flui-tree/src/arity/arity_storage.rs`,
  `test -f crates/flui-tree/src/arity/accessors.rs`
  are run sequentially
- THEN all three MUST exit non-zero

#### Scenario: Arity markers remain accessible

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{Leaf, Single, Optional, Variable, Arity};`
- THEN the build MUST succeed

#### Scenario: ChildrenStorage / ArityStorage symbols unresolvable

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{ChildrenStorage, ArityStorage, ChildrenAccess};`
- THEN the build MUST fail with unresolved imports

---

### Requirement: traits/node.rs (Node + NodeExt + NodeTypeInfo) is deleted

`crates/flui-tree/src/traits/node.rs` MUST NOT exist. The trait
`Node` (with associated `type Id: Identifier`), the `NodeExt`
extension trait (`type_name()`, `id_type_name()`), and
`NodeTypeInfo` MUST NOT exist in the crate's surface.

The crate's `lib.rs` MUST NOT declare `pub mod traits::node;`
(it does not, since `traits/mod.rs` controls the module's
children).

**Audit ref:** T-8 (closed Wave 4+5 — 305 LOC deleted; verdict
ratified as **permanent**).

**Flutter ref:** Flutter has no analogous `Node` trait. Each
tree class (Element, RenderObject, Layer, SemanticsNode) is its
own type with its own ID; FLUI's deleted `Node` was a generic
that no consumer implemented.

**Revival trigger:** A future cross-tree algorithm wants a
generic "trait Node { type Id }" bound. At that point, the
trait can be re-introduced as a simple trait alias for
`Identifier`-having types.

#### Scenario: traits/node.rs file does not exist

- GIVEN the repository at HEAD
- WHEN `test -f crates/flui-tree/src/traits/node.rs` is run
- THEN it MUST exit non-zero

#### Scenario: Node trait unresolvable

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::traits::Node;` OR
  `use flui_tree::Node;`
- THEN the build MUST fail with unresolved imports

---

### Requirement: TreeReadExt and TreeNavExt extension traits are kept (T-15 partial)

`TreeReadExt` and `TreeNavExt` extension traits MUST remain in the
crate's surface (re-exported via the prelude). These provide
ergonomic shortcuts (`find_node_where`, `count_nodes_where`,
`collect_nodes_where`, `for_each_node`, `find_child_where`,
`find_descendant_where`, `visit_subtree`, `count_descendants_where`,
`path_to_node`, `nth_child`, `first_and_last_child`) atop the
core `TreeRead` / `TreeNav` methods.

`MountableExt` (the sibling extension trait that lived in
`state.rs`) MUST NOT exist (deleted alongside `state.rs`).

**Audit ref:** T-15 partial (closed Wave 4+5 — `MountableExt`
deleted with `state.rs`; `TreeReadExt` / `TreeNavExt` kept per
cycle-3 disposition table: "have real-world ergonomic value").
Verdict ratified as **permanent**.

**Flutter ref:** None — extension traits are a Rust-native
ergonomic pattern; Flutter has per-class instance methods.

#### Scenario: TreeReadExt and TreeNavExt are publicly accessible

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{TreeReadExt, TreeNavExt};`
- THEN the build MUST succeed

#### Scenario: MountableExt is NOT accessible

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::MountableExt;`
- THEN the build MUST fail with unresolved import

---

### Requirement: Deferred audit finding T-17 — Slot::with_siblings positional signature kept (revisit-later-with-trigger)

`Slot::with_siblings(parent: I, index: usize, depth: Depth,
previous_sibling: Option<I>, next_sibling: Option<I>) -> Self`
MUST remain as a positional constructor. The audit's
`bon`-builder recommendation is deferred per cycle-3 deferral
rationale: "Builder pattern conversion is its own commit theme".

**Verdict for T-17:** **revisit-later-with-trigger**.
Revival trigger: a workspace-wide pass adopts `bon` builders
for multi-arg constructors elsewhere AND the cost of converting
`Slot::with_siblings` aligns with that pass. Recorded in
`crates/flui-tree/ARCHITECTURE.md ## Outstanding refactors`.

**Audit ref:** T-17 (deferred → revisit-later-with-trigger in
this spec).

#### Scenario: Slot::with_siblings remains a positional constructor

- GIVEN `crates/flui-tree/src/iter/slot.rs`
- WHEN searched for `pub fn with_siblings`
- THEN exactly one match MUST appear AND the next line MUST
  be a parameter list with `parent`, `index`, `depth`,
  `previous_sibling`, `next_sibling` (positional, not builder)

---

### Requirement: Deferred audit finding T-24 — iter::* constructors stay pub (revisit-later-with-trigger)

`Ancestors::new`, `AncestorsWithDepth::new`, `Descendants::new`,
`DescendantsWithDepth::new`, `Siblings::new`, `AllSiblings::new`
MUST remain `pub`. The audit's recommendation to downgrade to
`pub(crate)` is deferred per cycle-3 deferral rationale:
"Re-exported via `flui_tree::*` already; reducing visibility
breaks the iter API".

**Verdict for T-24:** **revisit-later-with-trigger**.
Revival trigger: workspace-wide audit shows every concrete
TreeNav impl constructs these via the trait method (not the
constructor) AND no consumer outside `flui-tree` itself uses
the constructor. Recorded in
`crates/flui-tree/ARCHITECTURE.md ## Outstanding refactors`.

**Audit ref:** T-24 (deferred → revisit-later-with-trigger in
this spec).

#### Scenario: Iterator constructors remain publicly accessible

- GIVEN a downstream test crate
- WHEN it attempts `use flui_tree::{Ancestors, Descendants, Siblings};`
  followed by `let _ = Ancestors::new(&some_tree, some_id);`
- THEN compilation MUST succeed (proves the constructors are
  publicly callable per T-24 deferral)

---

### Requirement: All cycle-3 deletions are recorded as permanent in tree-architecture-md

`crates/flui-tree/ARCHITECTURE.md ## Mapping decisions` MUST
record each cycle-3 deletion (the seven groups above:
`state.rs`, `visitor/`, `diff.rs`, four iterator files,
three arity-storage files, `traits/node.rs`,
`MountableExt`) as an "Accepted trade-off" entry with:
- Deleted surface (file:LOC summary).
- Rationale (memory rule `no-quick-wins-vanyastaff` + audit
  Appendix A.2 zero-consumer evidence).
- Revival trigger (the per-deletion trigger this spec
  enumerates above).

This cross-references the `tree-architecture-md` spec (which
owns the structural ARCHITECTURE.md author requirement).

**Audit ref:** Multiple (T-3 through T-8 + T-15 partial); all
verdicts ratified.

#### Scenario: ARCHITECTURE.md records each deletion

- GIVEN `crates/flui-tree/ARCHITECTURE.md` after the
  `tree-architecture-md` spec's task completes
- WHEN searched for "Accepted trade-off:" entries referencing
  `state.rs`, `visitor/`, `diff.rs`, `iter/cursor.rs`,
  `iter/path.rs`, `iter/breadth_first.rs`, `iter/depth_first.rs`,
  `arity/storage.rs`, `arity/arity_storage.rs`,
  `arity/accessors.rs`, `traits/node.rs`, `MountableExt`
- THEN every name in this list MUST appear in at least one
  `## Mapping decisions` entry
