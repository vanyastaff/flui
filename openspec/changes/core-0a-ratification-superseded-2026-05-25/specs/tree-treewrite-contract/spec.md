# Tree — TreeWrite Contract Specification

## Purpose

Pin the canonical contract for FLUI's `TreeWrite<I>` and
`TreeWriteNav<I>` traits — the mutation surface for every concrete
tree in the workspace (`RenderTree`, `LayerTree`, `SemanticsTree`,
the forthcoming `ElementTree`, `ViewTree`). The headline contract
is **cascade-by-default `remove` + opt-out `remove_shallow`**, lifted
to the trait level in cycle 3 PR #103 (Wave 1+2) so every adopter
inherits the safety guarantee instead of re-implementing it per
tree.

`flui-tree` has no direct Flutter file:line counterpart — Flutter's
tree mechanics live in `widgets/framework.dart::Element`. Each
requirement below either (a) maps to the framework.dart operation
it abstracts over, or (b) is explicitly marked **FLUI-only
construct** with the unified-trio justification.

Cycle 3 closed all 13 audit findings on this domain (T-1 through
T-14, plus PR #103 Codex P2 stack-safety review). This spec
records the cascade-contract guarantees so cycle 4+ does not
re-litigate them.

Owner crate: `crates/flui-tree` — module `traits/write.rs`.

## Requirements

### Requirement: TreeWrite::remove cascades to descendants by default

`TreeWrite<I: Identifier>::remove(&mut self, id: I) -> Option<Self::Node>`
MUST remove `id` AND every transitive descendant. The contract is
the trait's default implementation; impls MAY override for storage-
specific efficiency (e.g. arena-bulk-free) but MUST preserve the
cascade semantics.

Removal order MUST be **post-order**: leaves dispose before their
parents, root last. This guarantees that `LayerNode::Drop` (cycle 2
PR #100 U8) and any future engine-listener dispose hooks see
children-already-gone state.

`remove` MUST be a no-op (return `None`) for an `id` not present
in storage.

**Audit ref:** T-1 (closed Wave 1+2 — cascade-by-default trait
contract); T-2 (closed Wave 4+5 — LayerTree/SemanticsTree adopt
the trait, removing parallel APIs); PR #103 Codex P2 (closed
Wave 3 — iterative cascade replaces recursive `self.remove`).

**Flutter ref:** No direct foundation/dart equivalent;
`.flutter/packages/flutter/lib/src/rendering/layer.dart:783-822`
(`LayerHandle._unref` cascade pattern) and
`.flutter/packages/flutter/lib/src/widgets/framework.dart::Element.deactivateChild`
(Element-tree deactivation cascade) are the parity inspirations.
FLUI lifts the cascade to a generic trait so every tree gets it.

**Rust-native divergence:**
- (a) Flutter's cascade lives in per-class methods (LayerHandle,
  Element.deactivateChild, ContainerLayer.remove), each implemented
  independently with subtly different cascade ordering.
- (b) FLUI provides ONE cascade contract in `TreeWrite::remove` —
  every adopter inherits the same post-order walk. This is a
  Rust-native consolidation enabled by trait default impls (no
  Flutter analog).
- (c) Pre-cycle-3 the trait's `remove` was non-cascade (audit T-1
  footgun); this is a **deliberate breaking change** to the trait
  contract. The audit verified the only existing adopter at the
  time (`RenderTree`) had no orphaning-dependent callers.

#### Scenario: remove cascades down a three-level subtree

- GIVEN a `TestTree` with `root → child → grandchild` (3 nodes)
- WHEN `tree.remove(root)` is called
- THEN the call MUST return `Some(root_node)`; `tree.len()` MUST
  equal `0`; `tree.contains(root)`, `tree.contains(child)`, and
  `tree.contains(grandchild)` MUST all return `false`
- (verbatim regression test `remove_cascades_by_default` at
  `crates/flui-tree/src/traits/write.rs::tests`)

#### Scenario: remove of a missing id is a no-op

- GIVEN a `TestTree` with one node and a phantom `ElementId::new(999)`
- WHEN `tree.remove(phantom)` is called
- THEN the call MUST return `None`; `tree.len()` MUST be unchanged
  (1)

#### Scenario: remove of an empty subtree (single leaf) removes only that node

- GIVEN a `TestTree` with `root → child` (2 nodes) and call
  `tree.remove(child)`
- WHEN the call returns
- THEN `tree.contains(child)` MUST be `false`; `tree.contains(root)`
  MUST be `true`; `tree.len()` MUST equal `1`

---

### Requirement: TreeWrite::remove is iterative (stack-safe for deep trees)

The default implementation of `TreeWrite::remove` MUST use an
**explicit worklist `Vec<I>` on the heap**, NOT recursive `self.remove(child_id)`
calls. The walk MUST be safe for trees of arbitrary depth up to
`depth::MAX_TREE_DEPTH` without exhausting the native call stack.

Specifically, a linear chain of `≥ 2_000` nodes MUST be removable
in a single `remove(root)` call without stack overflow on any
target platform with default thread-stack budget.

**Audit ref:** PR #103 Codex P2 (closed Wave 3 — iterative
cascade replaces recursive shape; regression test
`remove_cascade_is_stack_safe_on_deep_chain` at
`crates/flui-tree/src/traits/write.rs:DEPTH = 2_000`).

**Flutter ref:** Flutter's Element-tree deactivation is iterative
(Dart's per-frame stack budget makes recursion equally risky).

**Rust-native divergence:** Same shape. The iterative walk is
universal-correctness, not divergence.

#### Scenario: 2 000-deep linear chain removes without stack overflow

- GIVEN a `TestTree` built as a 2 000-node linear chain (each node
  has exactly one child, except the leaf)
- WHEN `tree.remove(root)` is called
- THEN the call MUST return `Some(_)` without panic or stack
  overflow; `tree.len()` MUST equal 0
- (verbatim regression test
  `remove_cascade_is_stack_safe_on_deep_chain` at
  `crates/flui-tree/src/traits/write.rs::tests`)

---

### Requirement: TreeWrite::remove_shallow preserves the pre-cycle non-cascade behaviour

`TreeWrite<I>::remove_shallow(&mut self, id: I) -> Option<Self::Node>`
MUST remove ONLY the specified node. Descendants of the removed
node MUST remain in storage with their `parent_id` pointing at
the now-removed `id` (orphan state). Callers using
`remove_shallow` are responsible for re-attaching the descendants
to a new parent (or accepting the orphan state for testing /
inspection purposes).

`remove_shallow` MUST:
1. Unlink the node from its parent's children list (if any).
2. Update root tracking if `id` is the root.
3. Remove the node from storage and return it.
4. Leave descendants and their parent pointers intact.

`remove_shallow` is the trait's **required primitive**; the
default `remove` implementation builds on it. Every concrete
adopter (`RenderTree`, `LayerTree`, `SemanticsTree`, future
adopters) MUST implement `remove_shallow`.

**Audit ref:** T-1 (closed Wave 1+2 — remove_shallow opt-out
introduced alongside cascade-by-default `remove`).

**Flutter ref:** Flutter `Element.deactivateChild` is the partial
parity — deactivates but does not destroy (descendants live in
the inactive list awaiting reactivation via GlobalKey
reparenting). FLUI's `remove_shallow` is more aggressive
(removes the node from storage) but preserves the descendant-
intact invariant needed for reparenting workflows.

**Rust-native divergence:**
- (a) Flutter splits the operation across `deactivateChild`
  (single-node teardown) and the inactive-list lifecycle. FLUI
  exposes both shapes via the trait method pair.
- (b) `remove_shallow` is the deliberately-unsafe API for re-
  parenting workflows; the `remove` cascade is the safe
  default.
- (c) No consumer breaks — the trait method pair was introduced
  cleanly in cycle 3.

#### Scenario: remove_shallow leaves child orphaned in storage

- GIVEN a `TestTree` with `root → child` (2 nodes)
- WHEN `tree.remove_shallow(root)` is called
- THEN the call MUST return `Some(root_node)`; `tree.len()` MUST
  equal `1`; `tree.contains(child)` MUST return `true` (orphan
  state)
- (verbatim regression test `remove_shallow_does_not_cascade` at
  `crates/flui-tree/src/traits/write.rs::tests`)

#### Scenario: remove_shallow updates parent's children list

- GIVEN a `TestTree` with `root → [child_a, child_b]` (3 nodes)
  and call `tree.remove_shallow(child_a)`
- WHEN `tree.children(root).collect::<Vec<_>>()` is collected
- THEN the result MUST equal `vec![child_b]` (proves child_a was
  unlinked from root's children list)

#### Scenario: Every adopter implements remove_shallow

- GIVEN the workspace at HEAD
- WHEN `cargo check -p flui-rendering -p flui-layer -p flui-semantics`
  is run
- THEN exit code MUST be 0 (proves every `impl TreeWrite<I> for ...`
  block provides a `fn remove_shallow` body; the trait has no
  default for it)

---

### Requirement: TreeWrite::remove_subtree counts cascade size

`TreeWrite<I>::remove_subtree(&mut self, id: I) -> usize`
(where `Self: TreeNav<I> + Sized`) MUST:
1. Compute the count of nodes in the subtree rooted at `id`
   (including `id` itself) via the `descendants(id)` iterator.
2. Call `remove(id)` to cascade-delete the subtree.
3. Return the count of removed nodes (0 if `id` did not exist).

This is the convenience wrapper for devtools / messaging callers
who need an explicit "X nodes removed" report.

**Audit ref:** Mythos verdict (post-cycle ratification — the
method exists in `write.rs:178-189` and is part of the contract).

**Flutter ref:** No direct equivalent; Flutter computes such
counts ad-hoc per-call site.

#### Scenario: remove_subtree returns the cascade count

- GIVEN a `TestTree` with `root → child_1 → grandchild` and
  `root → child_2` (4 nodes total)
- WHEN `let count = tree.remove_subtree(child_1);`
- THEN `count` MUST equal `2` (child_1 + grandchild);
  `tree.len()` MUST equal `2` (root + child_2 survive)
- (verbatim regression test `test_remove_subtree` at
  `crates/flui-tree/src/traits/write.rs::tests`)

#### Scenario: remove_subtree of missing id returns 0

- GIVEN any `TestTree` and a phantom `ElementId::new(999)`
- WHEN `let count = tree.remove_subtree(phantom);`
- THEN `count` MUST equal `0`

---

### Requirement: TreeWrite blanket impls cover &mut T and Box<T>

The crate MUST provide `impl<I: Identifier, T: TreeWrite<I> + ?Sized> TreeWrite<I> for &mut T`
and `impl<I: Identifier, T: TreeWrite<I> + ?Sized> TreeWrite<I> for Box<T>`
forwarding implementations. Each forwarded method MUST delegate
to `(**self).method(...)`.

**Audit ref:** Mythos verdict (post-cycle ratification — blanket
impls in `write.rs:341-410`).

**Flutter ref:** None — Flutter uses interface inheritance instead
of blanket impls.

**Rust-native divergence:** Pure Rust idiom — the blanket impls
allow `&mut SomeTree` and `Box<dyn SomeTree>` to satisfy generic
`I: TreeWrite<...>` bounds.

#### Scenario: Boxed TreeWrite satisfies the trait bound

- GIVEN a function `fn run<I: TreeWrite<ElementId>>(t: &mut I)`
- WHEN it is called with `&mut Box::new(TestTree::new())`
- THEN compilation MUST succeed and the call MUST behave identically
  to calling on `&mut TestTree`

---

### Requirement: TreeWriteNav adds set_parent / add_child / detach / move_children / insert_child

`TreeWriteNav<I: Identifier>: TreeWrite<I> + TreeNav<I>` MUST
expose:
- `set_parent(child: I, new_parent: Option<I>) -> TreeResult<I>` —
  reparent (with cycle detection); the **only required method**
  (everything else has a default impl).
- `add_child(parent: I, child: I) -> TreeResult<I>` — convenience
  wrapper for `set_parent(child, Some(parent))`.
- `detach(child: I) -> TreeResult<I>` — convenience wrapper for
  `set_parent(child, None)`.
- `move_children(from: I, to: I) -> TreeResult<()>` — bulk-move
  every child of `from` to `to`; rejects cycles.
- `insert_child(node: Self::Node, parent: Option<I>) -> TreeResult<I>`
  — combines `insert` + `set_parent`; rolls back the insert with
  `remove_shallow` if `parent` does not exist (safe because the
  just-inserted node has no children yet).

`set_parent` MUST reject cycles (returning `TreeError::CycleDetected`)
when `new_parent` is `child` itself OR a descendant of `child`.

**Audit ref:** T-2 (closed Wave 4+5 — LayerTree/SemanticsTree
adopt these methods through the trait, removing parallel APIs).
T-13 (closed Wave 4+5 — `TreeError::ArityViolation` `#[from]
ArityError` unification).

**Flutter ref:** `.flutter/packages/flutter/lib/src/widgets/framework.dart`
(`Element.updateChild`, `Element.deactivateChild`, the reparenting
state machine).

**Rust-native divergence:**
- (a) Flutter's reparenting is bound to the Element-lifecycle
  state machine (`_lifecycleState`). FLUI's `TreeWriteNav` is the
  generic mutation surface; per-tree state machines (e.g.
  Element's Initial/Active/Inactive/Defunct FSM in `flui-view`)
  layer on top.
- (b) FLUI exposes `insert_child` as the rollback-safe combined
  primitive; Flutter has no such combined operation (each call
  site composes `_insert` + `_updateParent` manually).
- (c) No consumer breaks; the trait was extended cleanly.

#### Scenario: set_parent reparents a node and rejects cycles

- GIVEN a `TestTree` with `root → child` (2 nodes)
- WHEN `tree.set_parent(root, Some(child))` is called
  (would create a cycle)
- THEN the call MUST return `Err(TreeError::CycleDetected(_))`;
  `tree.parent(root)` MUST remain `None`
- (verbatim regression test `test_cycle_detection` at
  `crates/flui-tree/src/traits/write.rs::tests`)

#### Scenario: insert_child rolls back on missing parent

- GIVEN a `TestTree` with a single node `existing` and a
  phantom `ElementId::new(999)`
- WHEN `let result = tree.insert_child(TestNode::default(), Some(phantom));`
- THEN `result` MUST equal `Err(TreeError::NotFound(_))`; `tree.len()`
  MUST equal `1` (the speculative insert was rolled back via
  `remove_shallow`)

#### Scenario: move_children rejects cycle when 'to' is a descendant of 'from'

- GIVEN a `TestTree` with `root → child → grandchild` and call
  `tree.move_children(root, grandchild)`
- WHEN the call returns
- THEN it MUST equal `Err(TreeError::CycleDetected(_))`; the tree
  structure MUST be unchanged

#### Scenario: detach makes a child a root

- GIVEN a `TestTree` with `root → child`
- WHEN `tree.detach(child)` is called
- THEN `tree.parent(child)` MUST equal `None`; `tree.children(root).count()`
  MUST equal `0`
- (verbatim regression test `test_detach` at
  `crates/flui-tree/src/traits/write.rs::tests`)

---

### Requirement: TreeError carries ArityViolation via #[from] ArityError

`TreeError` MUST be `#[non_exhaustive]` and MUST expose a variant
`ArityViolation(#[from] ArityError)` so arity-rule failures (too
many children for a `Leaf` node, missing required child on a
`Single` node, etc.) flow through the unified tree-error type
without manual conversion at the call site.

**Audit ref:** T-13 (closed Wave 4+5 — `#[from] ArityError`
variant added; verdict ratified as **permanent**).

**Flutter ref:** Flutter wraps errors via `FlutterError`; FLUI's
typed-enum + `#[from]` is the Rust-native equivalent.

**Rust-native divergence:** Pure Rust idiom; no Flutter analog.

#### Scenario: ArityError converts to TreeError via ?

- GIVEN a function returning `TreeResult<()>` that calls a child
  function returning `Result<(), ArityError>`
- WHEN the inner function fails and `?` propagates
- THEN compilation MUST succeed (the `#[from] ArityError` impl
  provides the conversion)

---

### Requirement: All three production trees adopt TreeWrite<I>

`RenderTree` (`flui-rendering`), `LayerTree` (`flui-layer`), and
`SemanticsTree` (`flui-semantics`) MUST each implement `TreeRead<I>`,
`TreeNav<I>`, AND `TreeWrite<I>` for their respective ID family
(`RenderId`, `LayerId`, `SemanticsId`). The parallel mutation
APIs that pre-cycle-3 lived per-tree (LayerTree's own
`add_child` / `remove` / `remove_shallow`, SemanticsTree's
mirror) MUST NOT exist as separate methods — they are the
trait-provided defaults or trait-required `remove_shallow` /
`insert` / `set_parent`.

**Audit ref:** T-2 (closed Wave 4+5 — adoption + ~200 LOC
parallel-API dedup).

**Flutter ref:** Per-tree mutation methods in
`.flutter/packages/flutter/lib/src/rendering/layer.dart` and
`semantics.dart`. FLUI consolidates via the trait.

**Rust-native divergence:**
- (a) Flutter: parallel mutation APIs per tree.
- (b) FLUI: one trait, three adopters. Saves ~200 LOC of
  duplication across the cycle-2 work.
- (c) The adopters' previous `pub fn add_child(...)` methods (if
  any survive as wrappers) MAY be retained as `#[deprecated]`
  forwarders to the trait method — but the trait is the canonical
  surface.

#### Scenario: RenderTree implements TreeWrite<RenderId>

- GIVEN `crates/flui-rendering/src/storage/tree.rs`
- WHEN searched for `impl TreeWrite<RenderId> for RenderTree`
- THEN exactly one match MUST appear

#### Scenario: LayerTree implements TreeWrite<LayerId>

- GIVEN `crates/flui-layer/src/tree/`
- WHEN searched recursively for `impl TreeWrite<LayerId> for LayerTree`
- THEN exactly one match MUST appear

#### Scenario: SemanticsTree implements TreeWrite<SemanticsId>

- GIVEN `crates/flui-semantics/src/`
- WHEN searched recursively for `impl TreeWrite<SemanticsId> for SemanticsTree`
- THEN exactly one match MUST appear
