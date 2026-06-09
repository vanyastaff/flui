# Foundation Variance and Lifetime Specification

## Purpose

Pin the requirements for type-variance correctness in `Id<T>` and for
over-specified HRTB lifetime bounds in `flui-tree` predicate APIs.  Both
findings (F7 and F8) were missed by cycle-3 because the variance dimension was
absent from its audit method.

Owner crates: `crates/flui-foundation` (`id.rs`),
`crates/flui-tree` (`traits/read.rs`, `traits/nav.rs`).

---

## Requirements

### Requirement: Id<T> MUST use invariant PhantomData (F7)

`pub struct Id<T: Marker>` in `crates/flui-foundation/src/id.rs` MUST be
declared with `PhantomData<fn() -> T>` (invariant in `T`), NOT `PhantomData<T>`
(covariant in `T`).

**Rationale (Rustonomicon §3.10):** `PhantomData<T>` makes `Id<T>` covariant in
`T`.  With current markers (all uninhabited `'static` ZSTs), covariance is
harmless.  However, if a future `Marker` becomes lifetime-parameterized
(`Marker<'a>`), covariance would allow an `Id<&'long>` to be coerced to
`Id<&'short>` silently at the `unsafe Id::from_raw` boundary — a potential
use-after-free path.  The wgpu-inspired ID system that FLUI cites at `id.rs:8`
uses invariant phantoms for this reason.

`PhantomData<fn() -> T>` is invariant in `T`, costs zero bytes, and makes no
change to the generated code for current `'static` markers.

**Acceptance criterion:** SC20 — `grep -n "PhantomData<fn() -> T>"
crates/flui-foundation/src/id.rs` exits 0.

#### Scenario: Id<T> struct uses invariant phantom (SC20)

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD (after change)
- WHEN the `Id<T>` struct definition is inspected
- THEN it contains `PhantomData<fn() -> T>` and NOT `PhantomData<T>`

#### Scenario: Variance change does not break existing callers

- GIVEN the workspace at HEAD after the PhantomData change
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0
- AND all current markers (`'static` ZSTs) continue to work identically

#### Scenario: Future lifetime-parameterized marker would not coerce unsoundly

- GIVEN a hypothetical `struct Marker<'a>(PhantomData<&'a ()>)` implementing
  `Marker`
- WHEN Rust's type checker evaluates `Id<Marker<'long>>` vs
  `Id<Marker<'short>>`
- THEN they are NOT implicitly coercible (invariant PhantomData prevents the
  coercion that `PhantomData<T>` would permit)

---

### Requirement: TreeReadExt and TreeNavExt predicates MUST NOT use over-specified HRTB bounds (F8) [PRIMARY]

`TreeReadExt` methods in `crates/flui-tree/src/traits/read.rs` and `TreeNavExt`
methods in `crates/flui-tree/src/traits/nav.rs` that accept predicate closures
MUST use the simplified bound `P: FnMut(&Self::Node) -> bool` instead of the
HRTB form `for<'a> FnMut(&'a Self::Node) -> bool`.

**Why the HRTB form is over-specified:** `for<'a> FnMut(&'a Self::Node) -> bool`
quantifies over ALL possible lifetimes for the node reference.  This is only
necessary when the predicate must hold a borrow obtained in one call and use it
across a subsequent call with a different lifetime — a pattern that none of the
`find_node_where`, `find_descendants_where`, or sibling methods exhibit.  Each
predicate is invoked with `&node` where the borrow lasts exactly one call.
Rust 1.75+ lifetime elision rules make `FnMut(&Self::Node) -> bool` equivalent
in inference for all current and foreseeable call sites.

**This is a bound relaxation — callers compile unchanged.**  Adding `for<'a>` is
more restrictive on the caller; removing it never breaks a caller that compiles
with the restricted form.

**Acceptance criterion:** SC17 — `! grep -n "for<'a> FnMut"
crates/flui-tree/src/traits/read.rs crates/flui-tree/src/traits/nav.rs` exits 0.

Cross-referenced in: `foundation-rust-1.95-idioms/spec.md` (D4 idiom pattern),
`tree-soundness-and-idioms/spec.md` (flui-tree trait changes).

#### Scenario: HRTB form is absent from TreeReadExt and TreeNavExt (SC17)

- GIVEN `crates/flui-tree/src/traits/read.rs` and `nav.rs` at HEAD
- WHEN `grep -n "for<'a> FnMut"
  crates/flui-tree/src/traits/read.rs
  crates/flui-tree/src/traits/nav.rs` is run
- THEN it exits with code 1 (no matches)

#### Scenario: Existing predicate closures compile without modification

- GIVEN a predicate written as `|node: &SomeNode| node.value() > 0`
- WHEN passed to `tree.find_node_where(predicate)` after the bound is simplified
- THEN it compiles without requiring any additional lifetime annotation or
  turbofish from the caller

#### Scenario: Workspace compiles cleanly after bound simplification

- GIVEN all affected method signatures updated in `read.rs` and `nav.rs`
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0
