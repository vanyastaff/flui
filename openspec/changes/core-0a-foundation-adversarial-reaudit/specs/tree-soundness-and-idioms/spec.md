# Tree Soundness and Idioms Specification

## Purpose

Pin the requirements for defence-in-depth soundness and idiom improvements in
`flui-tree`: cycle-detection in the `TreeWrite::remove` cascade walk (F19), the
`SmallVec`-backed worklist replacing bare `Vec<I>` (F24), and the `.get()`
canonical path in `TreeWriteNav` (F30).  The HRTB simplification in
`TreeReadExt`/`TreeNavExt` (F8) is specced primarily in
`foundation-variance-lifetime/spec.md`; a cross-reference appears at the end of
this file.

Owner crates: `crates/flui-tree` (`traits/write.rs`, `traits/read.rs`,
`traits/nav.rs`, `error.rs`, `depth.rs`).

---

## Requirements

### Requirement: TreeWrite::remove cascade MUST detect cycles (F19) [PRIMARY]

`TreeWrite::remove` in `crates/flui-tree/src/traits/write.rs` MUST be
refactored to use a `HashSet<I>` visited set that detects cycles during cascade
traversal.  A new `try_remove` method MUST be added; the existing `remove` MUST
delegate to it.

**Severity justification (Codex verdict — P1, not P0):**
Corrupted cyclic trees are NOT reachable through the standard public API:
`add_child` rejects cycle-introducing operations (cycle-3 PR #100/#101).
A P0 classification would require corrupted storage to be attacker-reachable via
deserialization, plugin input, FFI, or `unsafe` slab manipulation.  As a
**defence-in-depth** measure (symmetric to the cycle-bound already on
`Ancestors::next` from cycle-3 T-12), this is P1: the guard prevents
OOM/hang in corrupted-tree scenarios without being exploitable through the
normal API.

**New API surface — `try_remove`:**
```rust
fn try_remove(&mut self, id: I) -> Result<Option<Self::Node>, TreeError>
```
- Returns `Ok(Some(node))` when the subtree rooted at `id` is successfully
  removed.
- Returns `Ok(None)` when `id` is not present in the tree.
- Returns `Err(TreeError::CycleDetected { root: I })` when a cycle is detected
  during cascade traversal.  At this point the tree MAY be in a partially-modified
  state; callers SHOULD treat it as corrupted.
- Emits `tracing::warn!(?id, "TreeWrite::remove cascade aborted — cycle suspected")` on cycle detection.

**Existing `remove` MUST be preserved:**
```rust
fn remove(&mut self, id: I) -> Option<Self::Node> {
    match self.try_remove(id) {
        Ok(opt) => opt,
        Err(_) => {
            tracing::warn!(?id, "TreeWrite::remove: cycle detected, returning None");
            None
        }
    }
}
```
The `Option<Self::Node>` return type is retained for backward compatibility.

**Required implementation shape:**
1. Pre-traversal: allocate `let mut visited: HashSet<I> = HashSet::new()` and
   a `SmallVec<[I; INLINE_TREE_DEPTH]>` worklist (see F24).
2. On each `to_visit.pop()`: if `visited.contains(&current)` → cycle detected →
   return `Err(TreeError::CycleDetected { root: id })`.
3. Otherwise: `visited.insert(current)`, push children of `current`.
4. No mutation of tree state until traversal completes (collect the full removal
   worklist first; then apply removals).

**`I: Hash + Eq` bound:** The `Identifier` supertrait already requires `Hash +
Eq` (confirmed in `crates/flui-tree/src/lib.rs`). Adding `HashSet<I>` requires no
new bounds on `try_remove`.

**`TreeError::CycleDetected` variant:** `crates/flui-tree/src/error.rs` MUST
gain a `CycleDetected { root: usize }` (or `CycleDetected`) variant.

**Acceptance criteria:**
- SC7 — `cargo test -p flui-tree cascade_cycle_detection` exits 0.
- SC8 — `grep -n "fn try_remove" crates/flui-tree/src/traits/write.rs` exits 0.

Cross-referenced in: `foundation-soundness/spec.md` (D1 soundness justification),
`foundation-test-coverage/spec.md` (D10 regression test requirement).

#### Scenario: Cyclic tree does not OOM or hang (MANDATORY — SC7)

- GIVEN a `TestTree` containing nodes A and B where A's children include B and
  B's children include A (a 2-node cycle — introduced via a test helper that
  bypasses the `add_child` cycle-check)
- WHEN `tree.try_remove(A)` is called
- THEN it returns `Err(TreeError::CycleDetected { .. })` within bounded time
  (no infinite loop, no allocator exhaustion)
- AND `tracing::warn!` is emitted with a message containing "cycle"

#### Scenario: try_remove returns Ok(Some) on a valid tree

- GIVEN a valid (acyclic) tree with node X having descendants D1, D2, D3
- WHEN `tree.try_remove(X)` is called
- THEN it returns `Ok(Some(x_node))`
- AND X, D1, D2, D3 are no longer present in the tree

#### Scenario: try_remove returns Ok(None) for an absent node

- GIVEN a tree that does NOT contain node Y
- WHEN `tree.try_remove(Y)` is called
- THEN it returns `Ok(None)`
- AND the tree is unchanged

#### Scenario: remove() delegates to try_remove and returns None on cycle

- GIVEN a corrupted cyclic tree
- WHEN `tree.remove(root_id)` is called (the existing public API)
- THEN it returns `None`
- AND `tracing::warn!` is emitted
- AND the process does NOT OOM or hang

#### Scenario: TreeError::CycleDetected variant exists

- GIVEN `crates/flui-tree/src/error.rs` at HEAD
- WHEN `grep -n "CycleDetected" crates/flui-tree/src/error.rs` is run
- THEN it exits with code 0 (the variant is defined)

#### Scenario: try_remove signature present in trait (SC8)

- GIVEN `crates/flui-tree/src/traits/write.rs` at HEAD
- WHEN `grep -n "fn try_remove" crates/flui-tree/src/traits/write.rs` is run
- THEN it exits with code 0

---

### Requirement: TreeWrite::remove cascade MUST use SmallVec for worklist (F24) [PRIMARY]

Both `worklist` and `to_visit` in the `TreeWrite::remove` cascade walk in
`crates/flui-tree/src/traits/write.rs` MUST be declared as
`SmallVec<[I; INLINE_TREE_DEPTH]>` where `INLINE_TREE_DEPTH` is the constant
from `crates/flui-tree/src/depth.rs` (value: 32).  Bare `Vec<I>` declarations
MUST NOT be used for these local variables.

**Rationale:** Cycle-3 T-10 established `INLINE_TREE_DEPTH = 32` as the SmallVec
sizing canon and applied it to LCA, `Ancestors`, and `Siblings`.  The cascade walk
in `write.rs` was added AFTER the T-10 sweep and never received the optimisation.
For typical subtrees (≤32 nodes — the common case in UI trees), `SmallVec`
keeps all allocations on the stack, producing zero allocator pressure on the
cascade hot path.

This change is combined with the F19 cycle-detection fix: both `Vec<I>`
declarations are replaced in the same edit.

**Acceptance criterion:** SC19 — `! grep -n "let mut worklist: Vec<I>"
crates/flui-tree/src/traits/write.rs` exits 0.

#### Scenario: Cascade worklist uses SmallVec not Vec (SC19)

- GIVEN `crates/flui-tree/src/traits/write.rs` at HEAD
- WHEN `grep -n "let mut worklist: Vec<I>\|let mut to_visit: Vec<I>"
  crates/flui-tree/src/traits/write.rs` is run
- THEN it exits with code 1 (no bare `Vec<I>` worklist declarations)

#### Scenario: SmallVec worklist uses INLINE_TREE_DEPTH constant

- GIVEN the worklist declarations in `try_remove` / `remove`
- WHEN the type annotations are inspected
- THEN they reference `INLINE_TREE_DEPTH` from `crates/flui-tree/src/depth.rs`
  (not a magic literal like `32`)

#### Scenario: Small-subtree removal incurs no heap allocation

- GIVEN a tree whose subtree rooted at X has ≤32 nodes
- WHEN `tree.try_remove(X)` is called
- THEN the cascade traversal requires no heap allocation from `worklist` or
  `to_visit` (both SmallVec inline capacities are sufficient)

---

### Requirement: TreeWriteNav MUST use Identifier::get() not Into<usize> (F30) [PRIMARY]

`TreeWriteNav::move_children` and `TreeWriteNav::insert_child` in
`crates/flui-tree/src/traits/write.rs` MUST use `id.get()` (the
`Identifier::get` canonical path) instead of `id.into()` at every site where a
`usize` is needed.  The `I: Into<usize>` bound MUST be removed from the `where`
clauses of these methods.

**Rationale:** Cycle-3 T-14 established `Identifier::get` as the canonical
`I → usize` conversion and made `From<Index> for Id<T>` always available (not
`#[cfg(test)]`).  The symmetric direction (`Id → usize`) was left with the legacy
`into()` form in two `TreeWriteNav` methods.  `id.get()` is explicit, does not
require an `Into<usize>` bound, and is consistent with the rest of `flui-tree`
after T-14.  This closes the T-14 partial closure.

**Acceptance criterion:** `! grep -n "\.into()" crates/flui-tree/src/traits/write.rs`
exits 0 for the `move_children` and `insert_child` bodies; `I: Into<usize>`
removed from both method `where` clauses.

Cross-referenced in: `foundation-rust-1.95-idioms/spec.md § Cross-references →
F30`.

#### Scenario: .into() calls replaced with .get() in move_children

- GIVEN `TreeWriteNav::move_children` at HEAD
- WHEN `grep -n "\.into()" crates/flui-tree/src/traits/write.rs` is run with the
  context of `move_children`
- THEN no `.into()` calls appear in the method body

#### Scenario: .into() calls replaced with .get() in insert_child

- GIVEN `TreeWriteNav::insert_child` at HEAD
- WHEN the method body is inspected
- THEN all `usize` conversions use `.get()` not `.into()`

#### Scenario: Into<usize> bound removed from move_children where clause

- GIVEN `TreeWriteNav::move_children` at HEAD
- WHEN the method's `where` clause is inspected
- THEN `I: Into<usize>` does NOT appear

#### Scenario: Workspace compiles after bound removal

- GIVEN `I: Into<usize>` removed from both methods
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0 (all call sites use `id.get()` which is provided by
  the `Identifier` supertrait)

---

## Cross-reference

### F8 — HRTB simplification in TreeReadExt / TreeNavExt

The primary requirement for dropping the `for<'a>` HRTB quantifier in
`TreeReadExt::find_node_where` and `TreeNavExt` sibling methods is specified in
`foundation-variance-lifetime/spec.md § Requirement: TreeReadExt and TreeNavExt
predicates MUST NOT use over-specified HRTB bounds`.  The changes land in
`crates/flui-tree/src/traits/read.rs` and `nav.rs` — the same crate as this
spec — so the task that implements F19 + F24 + F30 MAY bundle the F8 edit.
