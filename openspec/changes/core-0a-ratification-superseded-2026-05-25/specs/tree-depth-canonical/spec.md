# Tree — Canonical Depth Specification

## Purpose

Pin the canonical contract for FLUI's tree-depth abstraction —
`Depth`, `AtomicDepth`, `DepthAware`, `DepthError`, and the two
load-bearing constants `MAX_TREE_DEPTH` (validation cap) and
`INLINE_TREE_DEPTH` (`SmallVec` sizing hint). Pre-cycle-3 the
workspace had **four** different depth constants drifting
independently (`MAX_TREE_DEPTH=256`, `TreeNav::MAX_DEPTH=32`,
`TreeVisitor::MAX_STACK_DEPTH=64`, `TreeVisitorMut::STACK_SIZE=48`)
plus per-iterator inline-32 / inline-8 hard-codes. Cycle 3 Wave 3
(closing audit T-10, T-11, T-12, T-18, T-20) consolidated all of
this behind two constants in `crates/flui-tree/src/depth.rs`.

This spec records the single-source-of-truth invariant so future
contributions cannot re-fragment the constants.

Owner crate: `crates/flui-tree` — module `depth.rs`.

## Requirements

### Requirement: MAX_TREE_DEPTH is the sole validation cap (single source of truth)

`pub const MAX_TREE_DEPTH: usize = 256;` MUST be declared exactly
once in `crates/flui-tree/src/depth.rs` and MUST be the only
"maximum tree depth" cap anywhere in the workspace.

No other file in `crates/flui-tree/src/` MAY declare a const named
`MAX_TREE_DEPTH`, `MAX_DEPTH`, `MAX_STACK_DEPTH`, `STACK_SIZE`,
`STACK_DEPTH`, or `MARK_PROPAGATION_MAX_DEPTH` with a different
literal value. (Re-exports of `depth::MAX_TREE_DEPTH` are
permitted; independent literal definitions are forbidden.)

Downstream consumers of "maximum tree depth" (the iterator
size_hint upper bound, the visitor stack-size hint, any future
DAG-walk algorithm) MUST reference `flui_tree::depth::MAX_TREE_DEPTH`
rather than re-declaring the literal.

**Audit ref:** T-10 (closed Wave 3 — depth constants consolidated;
verdict ratified as **permanent**).

**Flutter ref:** None — Flutter has no analogous depth cap
(Element trees rely on Dart's default stack budget for recursion
limits).

**Rust-native divergence:**
- (a) Flutter: implicit "however deep before Dart stack overflow".
- (b) FLUI: explicit `MAX_TREE_DEPTH` cap because Rust iterators
  carry `size_hint` and `SmallVec` carries inline-capacity bounds
  that need explicit values. Audit's cycle 2 PR #101 was the
  cautionary tale (`MARK_PROPAGATION_MAX_DEPTH = 32` was too
  shallow; this spec prevents re-occurrence).
- (c) No consumer breaks; the constant has been at `256` since
  the audit and matches the cycle-3 consolidation.

#### Scenario: Exactly one MAX_TREE_DEPTH declaration in flui-tree

- GIVEN `crates/flui-tree/src/`
- WHEN searched recursively for `pub const MAX_TREE_DEPTH`
  (exact match on the declaration form)
- THEN exactly one match MUST appear, in `depth.rs`

#### Scenario: No drifted depth constants in flui-tree

- GIVEN `crates/flui-tree/src/`
- WHEN searched recursively for `const (MAX_DEPTH|MAX_STACK_DEPTH|STACK_SIZE|STACK_DEPTH|MARK_PROPAGATION_MAX_DEPTH)`
  (regex; case-sensitive)
- THEN zero matches MUST appear that are not re-exports of
  `depth::MAX_TREE_DEPTH` or `depth::INLINE_TREE_DEPTH` (use a
  CI gate or `port-check.sh` rule for this)

---

### Requirement: INLINE_TREE_DEPTH is the sole SmallVec sizing hint for depth-bounded operations

`pub const INLINE_TREE_DEPTH: usize = 32;` MUST be declared exactly
once in `crates/flui-tree/src/depth.rs`. The value MUST be ≤
`MAX_TREE_DEPTH` (currently 32 ≤ 256).

Every `SmallVec<[T; N]>` whose `N` represents tree depth (ancestor
stacks, descendant stacks, lowest-common-ancestor scratch lists,
sibling buffers, path-to-node lists) MUST use `INLINE_TREE_DEPTH`
as the inline cap, NOT a hard-coded `[T; 8]` / `[T; 16]` / `[T; 32]`
/ `[T; 48]` / `[T; 64]` literal.

The two known consumers are:
- `Ancestors` / `AncestorsWithDepth` iterators
  (`iter/ancestors.rs`) — depth-bounded by `INLINE_TREE_DEPTH`.
- `Descendants` / `DescendantsWithDepth` iterators
  (`iter/descendants.rs`) — descendant stack sized by
  `INLINE_TREE_DEPTH`.
- `Siblings` iterator (`iter/siblings.rs`) — sibling buffer
  sized by `INLINE_TREE_DEPTH` (T-20 close).
- `TreeNavExt::lowest_common_ancestor` (`traits/nav.rs`) — both
  ancestor-list scratch vectors sized by `INLINE_TREE_DEPTH`
  (T-18 close).

**Audit ref:** T-10 (closed Wave 3 — single-source sizing hint);
T-18 (closed Polish PR — `lowest_common_ancestor` adopts
`SmallVec<[I; INLINE_TREE_DEPTH]>`); T-20 (closed Polish PR —
`Siblings::new` adopts `SmallVec`). All ratified as **permanent**.

**Flutter ref:** None — Flutter has no `SmallVec` analog (Dart's
`List<T>` is uniformly heap-allocated with growth).

**Rust-native divergence:**
- (a) Flutter: every list is heap-allocated.
- (b) FLUI: `SmallVec<[T; INLINE_TREE_DEPTH]>` stack-allocates
  the common case (depth ≤ 32) and falls back to heap for
  pathologically deep trees up to `MAX_TREE_DEPTH`.
- (c) The value `32` is the cycle-3 choice based on the audit's
  observation that typical widget/element/view trees rarely
  exceed depth 32 (per Flutter's `Element.depth` survey). No
  consumer breaks; SmallVec heap fallback covers overflow.

#### Scenario: INLINE_TREE_DEPTH ≤ MAX_TREE_DEPTH

- GIVEN the workspace at HEAD
- WHEN a compile-time assertion is added to `depth.rs`:
  `const _: () = { assert!(INLINE_TREE_DEPTH <= MAX_TREE_DEPTH); };`
- THEN compilation MUST succeed

#### Scenario: No hard-coded SmallVec inline caps for depth-related arrays in flui-tree

- GIVEN `crates/flui-tree/src/iter/` and `crates/flui-tree/src/traits/`
- WHEN searched for `SmallVec<\[.+; (8|16|32|48|64)\]>`
- THEN zero matches MUST appear that are not also matched by the
  literal `INLINE_TREE_DEPTH` token in the same expression
  (i.e. allow `SmallVec<[T; INLINE_TREE_DEPTH]>` whose literal
  expansion happens to be 32, but reject `SmallVec<[T; 32]>`
  written directly)

---

### Requirement: Depth is a #[repr(transparent)] wrapper over usize

`Depth` MUST be declared `#[repr(transparent)]` over `usize` so it
has zero memory overhead. `std::mem::size_of::<Depth>()` MUST equal
`std::mem::size_of::<usize>()`.

`Depth` MUST expose:
- `const fn root() -> Self` — depth-0 constructor.
- `const fn new(value: usize) -> Self` — direct constructor (no
  validation; saturates by NOT panicking on `≥ MAX_TREE_DEPTH`).
- `fn new_checked(value: usize) -> Result<Self, DepthError>` —
  validation-style constructor; returns `Err(DepthExceeded)` if
  `value > MAX_TREE_DEPTH`.
- `const fn get(self) -> usize` — extract the inner value.
- `const fn is_root(self) -> bool` — equivalent to `self.get() == 0`.
- `const fn child_depth(self) -> Self` — `self + 1` saturating at
  `usize::MAX`.
- `const fn distance_to(self, other: Self) -> usize` — absolute
  difference.

**Audit ref:** Mythos verdict (`Depth` + `AtomicDepth` are
"Don't touch"; cycle 3 audit explicitly says the shape is right,
only the constant fragmentation needed fixing).

**Flutter ref:** `.flutter/packages/flutter/lib/src/widgets/framework.dart::Element._depth`
(Flutter uses an `int` field directly; FLUI's typed wrapper is a
**Rust-native improvement**).

**Rust-native divergence:**
- (a) Flutter: `int _depth` on `Element`. No semantic distinction
  from any other `int`.
- (b) FLUI: `Depth` newtype with semantic methods (`child_depth`,
  `distance_to`, `is_root`). Same memory cost (1 `usize`), better
  type safety.
- (c) No consumer breaks; the wrapper has been stable since
  pre-cycle-1.

#### Scenario: Depth has zero memory overhead

- GIVEN runtime check
- WHEN `assert_eq!(std::mem::size_of::<Depth>(), std::mem::size_of::<usize>())`
  is asserted
- THEN the assertion MUST pass

#### Scenario: child_depth saturates

- GIVEN `let max = Depth::new(usize::MAX);`
- WHEN `max.child_depth()` is called
- THEN the returned value MUST equal `Depth::new(usize::MAX)` (no
  panic, no wraparound)

#### Scenario: new_checked rejects above-cap values

- GIVEN no prior context
- WHEN `Depth::new_checked(MAX_TREE_DEPTH + 1)` is called
- THEN it MUST return `Err(DepthError::DepthExceeded { ... })`

---

### Requirement: AtomicDepth provides thread-safe depth tracking

`AtomicDepth` MUST be the `Send + Sync` thread-safe counterpart to
`Depth`. It MUST expose:
- `fn root() -> Self` — depth-0 constructor.
- `fn new(d: Depth) -> Self` — initialiser from `Depth`.
- `fn get(&self) -> Depth` — `load(Acquire)`.
- `fn set(&self, d: Depth)` — `store(Release, ...)`.
- The standard `Acquire`/`Release` ordering pair for the common
  case (matches *Rust Atomics and Locks* §3 guidance).

The internal storage MUST be `AtomicUsize` to preserve niche-
optimisation and atomic-instruction compatibility on every target.

**Audit ref:** Mythos verdict (`AtomicDepth` is "Don't touch").

**Flutter ref:** None — Flutter is single-threaded; no atomic
analog.

**Rust-native divergence:** Pure Rust-native; cross-thread depth
tracking is a Rust idiom required by FLUI's multi-threaded
runtime.

#### Scenario: AtomicDepth round-trips with Acquire/Release

- GIVEN `let a = AtomicDepth::new(Depth::new(5));`
- WHEN `a.set(Depth::new(10));` is called from one thread,
  followed by `let v = a.get();` on another thread (joined via
  scoped thread)
- THEN `v` MUST equal `Depth::new(10)` (proves Acquire/Release
  happens-before edge)

---

### Requirement: ROOT_DEPTH is the depth-0 constant

`pub const ROOT_DEPTH: usize = 0;` MUST be declared in
`crates/flui-tree/src/depth.rs`. `Depth::root().get() == ROOT_DEPTH`
MUST hold.

**Audit ref:** Mythos verdict (post-cycle ratification).

#### Scenario: ROOT_DEPTH equals 0

- GIVEN no prior context
- WHEN `Depth::root().get() == ROOT_DEPTH` is evaluated
- THEN it MUST equal `true`

---

### Requirement: DepthAware trait exposes a node's depth

`pub trait DepthAware { fn depth(&self) -> Depth; }` MUST be the
public trait that depth-aware nodes implement so cross-tree
algorithms can ask for a node's depth without per-tree knowledge.

**Audit ref:** Mythos verdict (post-cycle ratification).

**Flutter ref:** `.flutter/packages/flutter/lib/src/widgets/framework.dart`
(`Element.depth` getter — class-level method on Element only;
FLUI's `DepthAware` is the generic equivalent).

#### Scenario: DepthAware is implementable by any node type

- GIVEN a hypothetical test node `struct TestNode { d: Depth }`
- WHEN the file declares `impl DepthAware for TestNode { fn depth(&self) -> Depth { self.d } }`
- THEN compilation MUST succeed

---

### Requirement: Deferred audit finding T-19 — TreeNav::depth default impl stays O(depth) (revisit-later-with-trigger)

`TreeNav<I>::depth(&self, id: I) -> usize` default implementation
MAY walk the `ancestors(id)` iterator and `count()` (O(depth)
cost per call). Consumers with stored-depth nodes (e.g.
`RenderTree` where `RenderNode` carries a `depth: AtomicDepth`
field) SHOULD override with an O(1) implementation.

The default-impl shape is the deferred T-19 verdict: doc-only
fix per the cycle-3 deferral table.

**Verdict for T-19:** **revisit-later-with-trigger**. Doc-comment
on the default impl MUST explicitly warn: `// SLOW DEFAULT:
O(depth) — Slab-based impls should override.`

Revival trigger: a profile shows `TreeNav::depth` is a hotspot
on an impl that has stored depth and forgot to override OR a
doc-cleanup pass standardises the slow-default markings across
the trait surface. Recorded in
`crates/flui-tree/ARCHITECTURE.md ## Outstanding refactors`.

**Audit ref:** T-19 (deferred → revisit-later-with-trigger in
this spec).

#### Scenario: Default impl exists and is documented as slow

- GIVEN `crates/flui-tree/src/traits/nav.rs`
- WHEN inspected near `fn depth(&self, id: I) -> usize`
- THEN the default impl MUST exist (per audit
  T-19 — keep the default for ergonomics) AND its doc-comment
  MUST mention "O(depth)" or "slow default" (the warning
  enforces the deferral verdict's "doc-only" mitigation)

#### Scenario: RenderTree overrides depth with O(1) implementation

- GIVEN `crates/flui-rendering/src/storage/tree.rs`
- WHEN searched for `fn depth(` inside the
  `impl TreeNav<RenderId> for RenderTree` block
- THEN exactly one match MUST appear (proves the override is
  in place, satisfying the cycle-3 close on T-9's sibling
  T-19 hot-path concern)

---

### Requirement: Descendants iterator is loop-based, not recursive

`Descendants::next(&mut self)` (and `DescendantsWithDepth::next`)
MUST use a `loop { ... continue; }` pattern when skipping missing
children, NOT `return self.next();` recursion. This guarantees
constant stack usage regardless of how many missing entries the
iterator encounters in pathological trees.

**Audit ref:** T-11 (closed Wave 3 — `Descendants::next` rewritten
as loop; verdict ratified as **permanent**).

**Flutter ref:** `.flutter/packages/flutter/lib/src/rendering/layer.dart:457+`
(`_depthFirstWalkChildren` uses explicit loop — same shape).

#### Scenario: Descendants::next handles missing-entry-loop without stack growth

- GIVEN a `TestTree` where the parent's `children` list contains
  100 IDs, none of which exist in storage
- WHEN `tree.descendants(parent).count()` is evaluated
- THEN the call MUST return `0` without stack overflow (proves
  the loop-based skip; pre-cycle-3 recursive shape would have
  consumed 100 stack frames)

#### Scenario: source code is loop-based

- GIVEN `crates/flui-tree/src/iter/descendants.rs`
- WHEN searched for `return self.next()`
- THEN zero matches MUST appear (proves the loop-rewrite is
  canonical; recursive shape is forbidden)

---

### Requirement: Ancestors iterator caps walks at MAX_TREE_DEPTH (cycle defense)

`Ancestors::next(&mut self)` (and `AncestorsWithDepth::next`) MUST
include a step counter bounded by `MAX_TREE_DEPTH` so a malformed
tree with a parent-cycle does not loop forever. On exceeding the
cap the iterator MUST end with `None` (with a `debug_assert!` in
debug builds).

**Audit ref:** T-12 (closed Wave 3 — step counter added; verdict
ratified as **permanent**).

**Flutter ref:** Flutter relies on Dart stack overflow as the
implicit cap; FLUI's explicit cap is a defensive Rust-native
addition.

#### Scenario: Ancestors iterator on a parent-cycle terminates at MAX_TREE_DEPTH

- GIVEN a `TestTree` deliberately constructed with `a.parent = b`
  AND `b.parent = a` (parent-cycle)
- WHEN `tree.ancestors(a).count()` is evaluated
- THEN the count MUST be ≤ `MAX_TREE_DEPTH + 1` (proves the cap;
  pre-cycle-3 the iterator would loop forever)
