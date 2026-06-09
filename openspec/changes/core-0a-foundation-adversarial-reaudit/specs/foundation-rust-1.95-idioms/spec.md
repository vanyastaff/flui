# Foundation Rust 1.95 Idioms Specification

## Purpose

Pin the edition-2024 / Rust 1.81–1.95 idiom baseline for `flui-foundation` and
`flui-tree`.  This spec covers eight distinct idiom gaps: lint-attribute
modernisation (`#[allow]` → `#[expect]`), blanket allow → per-site expect in
flui-tree, cross-crate clippy policy alignment, visibility downgrade for
unexported types (closing cycle-3 I-10), doc-comment println! elimination,
redundant trait-bound removal (`Identifier: Into<Index>`), custom assert-macro
deletion, and the `.get()` canonical path for `I → usize` conversions in
`flui-tree`.

Owner crates: `crates/flui-foundation` (`id.rs`, `key.rs`, `notifier.rs`,
`assert.rs`, `lib.rs`), `crates/flui-tree` (`lib.rs`, `traits/write.rs`),
`crates/flui-scheduler` (`id.rs`).

---

## Requirements

### Requirement: #![allow(unsafe_code)] MUST be replaced with #![expect(unsafe_code)] (F9)

Every module-level `#![allow(unsafe_code)]` declaration in
`crates/flui-foundation/src/id.rs` and `crates/flui-foundation/src/key.rs` MUST
be replaced with:
```rust
#![expect(unsafe_code, reason = "<one-liner explaining why unsafe is required>")]
```

**Rationale:** `#[expect(<lint>)]` (stabilised Rust 1.81) emits
`unfulfilled_lint_expectation` when the lint no longer triggers.  `#[allow]`
silently rots: if F2's fix removes `NonZeroU64::new_unchecked` from `key.rs`,
the `#![allow(unsafe_code)]` becomes dead but raises no warning.
`#![expect(unsafe_code)]` forces the attribute to be removed in lockstep with
the unsafe code it covers.

**Acceptance criterion:** SC9 — `! grep -n "^#!\[allow(unsafe_code"
crates/flui-foundation/src/{id,key}.rs` exits 0.

#### Scenario: Module-level allow absent from key.rs (SC9 / key.rs half)

- GIVEN `crates/flui-foundation/src/key.rs` at HEAD
- WHEN `grep -n "^#!\[allow(unsafe_code" crates/flui-foundation/src/key.rs` runs
- THEN it exits with code 1 (no module-level `allow`)

#### Scenario: Module-level allow absent from id.rs (SC9 / id.rs half)

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD
- WHEN `grep -n "^#!\[allow(unsafe_code" crates/flui-foundation/src/id.rs` runs
- THEN it exits with code 1 (no module-level `allow`)

#### Scenario: expect surfaces lint when the unsafe is gone

- GIVEN `#![expect(unsafe_code, reason = "...")]` is present in `key.rs`
- WHEN the last `unsafe` block is removed from `key.rs`
- WHEN `cargo check -p flui-foundation` is run
- THEN a `unfulfilled_lint_expectation` warning is emitted at the `#[expect]`
  attribute, signalling that the attribute must be deleted

---

### Requirement: flui-tree MUST NOT use blanket #![allow] for clippy lints (F21)

`crates/flui-tree/src/lib.rs` MUST NOT contain a crate-level
`#![allow(clippy::module_name_repetitions)]` or
`#![allow(clippy::too_many_lines)]`.  Functions that legitimately require
suppression MUST carry per-function:
```rust
#[expect(clippy::too_many_lines, reason = "<concise rationale>")]
```

**Rationale:** Blanket `#![allow]` suppresses every occurrence of the lint
across the entire crate, hiding new violations added after the suppression.
Per-function `#[expect]` scopes suppression to exactly the code that needs it
and emits `unfulfilled_lint_expectation` if the function is later refactored
below the threshold, preventing attribute rot.

#### Scenario: Blanket allow is absent from flui-tree/src/lib.rs

- GIVEN `crates/flui-tree/src/lib.rs` at HEAD
- WHEN `grep -n "#!\[allow(clippy::too_many_lines\|clippy::module_name_repetitions"
  crates/flui-tree/src/lib.rs` is run
- THEN it exits with code 1 (no blanket allow)

#### Scenario: Per-function expect is present where needed

- GIVEN a function in `flui-tree` whose body exceeds the
  `clippy::too_many_lines` threshold after this change
- WHEN `cargo clippy --workspace -- -D warnings` is run
- THEN the function carries `#[expect(clippy::too_many_lines, reason = "...")]`
  and clippy exits 0

---

### Requirement: clippy::pedantic MUST be enabled in both flui-foundation and flui-tree (F22)

`crates/flui-foundation/src/lib.rs` MUST include `clippy::pedantic` in its
`#![warn(...)]` stack, matching the existing policy in
`crates/flui-tree/src/lib.rs`.

**Rationale:** A sibling foundation-layer crate pair SHOULD have a consistent
clippy policy.  Inconsistency means pedantic-level bugs detectable in `flui-tree`
go undetected in `flui-foundation` because the lint level differs.  The target
state is both crates warn on pedantic; new violations in both crates are
suppressed per-site with `#[expect]`.

#### Scenario: clippy::pedantic warn present in foundation/lib.rs

- GIVEN `crates/flui-foundation/src/lib.rs` at HEAD
- WHEN `grep -n "clippy::pedantic" crates/flui-foundation/src/lib.rs` is run
- THEN it exits with code 0 (at least one match)

#### Scenario: Workspace compiles without new warnings after pedantic enable

- GIVEN `clippy::pedantic` added to `flui-foundation/src/lib.rs` warn stack
- WHEN `cargo clippy --workspace -- -D warnings` is run
- THEN it exits with code 0 (any newly-triggered pedantic findings are already
  suppressed with per-site `#[expect(..., reason = "...")]`)

---

### Requirement: RawId and Index MUST be pub(crate) in flui-foundation (F23)

`pub struct RawId` in `crates/flui-foundation/src/id.rs` MUST be downgraded to
`pub(crate) struct RawId`.  `pub type Index = usize` MUST be downgraded to
`pub(crate) type Index = usize`.  The cycle-3 I-10 deferral reason ("scheduler
re-exports") is invalid: grep confirms `Index` and `RawId` appear in
`crates/flui-scheduler/src/id.rs` as imports but are unused in the scheduler
implementation body (clippy `unused_imports` would flag them).

**Prerequisite:** The unused `Index` and `RawId` imports MUST be removed from
`crates/flui-scheduler/src/id.rs` before the visibility downgrade.

**Rust-native change:**
- (a) `RawId` and `Index` become `pub(crate)` in `flui-foundation`.
- (b) Reduces foundation's public API surface; unexported implementation details
  should not be `pub`.
- (c) No external workspace consumer uses these types (confirmed by workspace grep
  — only the scheduler import exists, and it is unused in the impl body).

#### Scenario: RawId is not top-level pub in flui-foundation

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD
- WHEN `grep -n "^pub struct RawId" crates/flui-foundation/src/id.rs` is run
- THEN it exits with code 1 (no top-level `pub struct RawId`)

#### Scenario: Index is not top-level pub type in flui-foundation

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD
- WHEN `grep -n "^pub type Index" crates/flui-foundation/src/id.rs` is run
- THEN it exits with code 1 (no top-level `pub type Index`)

#### Scenario: Scheduler no longer imports RawId or Index

- GIVEN `crates/flui-scheduler/src/id.rs` at HEAD
- WHEN `grep -n "\bRawId\b\|\bIndex\b" crates/flui-scheduler/src/id.rs` is run
- THEN it exits with code 1 (references removed)

#### Scenario: Workspace compiles after visibility downgrade

- GIVEN `RawId` and `Index` downgraded to `pub(crate)` in `flui-foundation`
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0

---

### Requirement: Doc-comment examples MUST NOT use println! (F26)

All doc-comment examples (`///` and `//!` blocks) in
`crates/flui-foundation/src/` MUST NOT use `println!`, `eprintln!`, or `dbg!`.
Logging examples MUST use `tracing::info!`; examples that need a side-effect
placeholder MUST use `let _ = ...` or a `// ...` comment.

**Rationale:** Constitution Principle 6 (AGENTS.md) prohibits `println!` in
shipped code.  Doc-comments compile and run as doctests via
`cargo test --doc` (which is part of `just ci`).  Doctests ARE shipped code.

**Acceptance criterion:** SC10 — `! grep -rn "println!" crates/flui-foundation/src/` exits 0.

#### Scenario: No println! in foundation doctests (SC10)

- GIVEN `crates/flui-foundation/src/` at HEAD
- WHEN `grep -rn "println!" crates/flui-foundation/src/` is run
- THEN it exits with code 1 (no matches)

#### Scenario: Listener examples use tracing::info! not println!

- GIVEN a doc-comment in `notifier.rs` that demonstrates adding a listener
- WHEN the example code is inspected
- THEN it uses `tracing::info!("Value changed!")` (or equivalent) NOT
  `println!("Value changed!")`

---

### Requirement: Identifier MUST NOT require Into<Index> as a supertrait bound (F28)

`pub trait Identifier` in `crates/flui-foundation/src/id.rs` MUST NOT include
`Into<Index>` in its supertrait list.  Callers that need a `usize` from an
identifier MUST use `identifier.get()` (the canonical conversion path already
exposed by the trait).

**Rationale:** `id.into()` (via `Into<Index>`) and `id.get()` (via
`Identifier::get`) both produce `usize`.  Two API paths for the same conversion
increase cognitive cost.  `get()` is explicit and searchable; `into()` is
implicit and depends on type-context inference.  The `Into<Index>` supertrait
bound is redundant given the `get()` method.

**Rust-native change:**
- (a) Removes `Into<Index>` from the `Identifier` supertrait list.
- (b) `get()` is the canonical path; the trait surface is simpler.
- (c) Callers that wrote `let n: usize = id.into()` will get a compile error;
  migration: `let n: usize = id.get()`.

**Acceptance criterion:** SC21.

#### Scenario: Identifier trait definition lacks Into<Index> bound

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD
- WHEN the `pub trait Identifier` definition is inspected
- THEN `Into<Index>` does NOT appear in the supertrait list or the `where` clause
  of the trait definition

#### Scenario: Workspace callers migrated to id.get()

- GIVEN all workspace callers of `id.into()` (where the target is `usize` /
  `Index`) updated to `id.get()`
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0

---

### Requirement: debug_assert_valid! and sibling macros MUST be deleted (F29)

The macros `debug_assert_valid!`, `debug_assert_range!`, `debug_assert_finite!`,
and `debug_assert_not_nan!` in `crates/flui-foundation/src/assert.rs` MUST be
deleted.  All consumers MUST be replaced with `debug_assert!(condition,
"message")` from the Rust standard library.

**Rationale:** These macros expand to exactly what `debug_assert!` already
provides: a conditional panic that is compiled out in release.  They add no
FLUI-specific semantics — no extra telemetry, no custom message prefix, no
lint.  Cycle-3 I-14 closed the analogous `report_error!` / `report_warning!`
macros; this closes the remaining assert macros on the same axis.  Deletion
reduces the `assert.rs` file by ~80 LOC and eliminates a maintenance surface.

**Acceptance criterion:** SC11 — `! grep -n "macro_rules! debug_assert_valid\|debug_assert_range\|debug_assert_finite\|debug_assert_not_nan"
crates/flui-foundation/src/assert.rs` exits 0.

#### Scenario: Custom assert macros are absent (SC11)

- GIVEN `crates/flui-foundation/src/assert.rs` at HEAD
- WHEN `grep -n "macro_rules! debug_assert_valid"
  crates/flui-foundation/src/assert.rs` is run
- THEN it exits with code 1 (no matches)

#### Scenario: All four macro names absent from assert.rs

- GIVEN `crates/flui-foundation/src/assert.rs` at HEAD
- WHEN `grep -n "debug_assert_valid\|debug_assert_range\|debug_assert_finite\|debug_assert_not_nan"
  crates/flui-foundation/src/assert.rs` is run
- THEN it exits with code 1 (no macro definitions or uses remain)

#### Scenario: Workspace compiles after macro deletion

- GIVEN all former callers of the deleted macros updated to `debug_assert!`
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0

---

## Cross-references

### F8 — HRTB simplification in tree traits (D4 idiom aspect)

The primary requirement for dropping the `for<'a>` quantifier in
`TreeReadExt`/`TreeNavExt` predicate bounds is specified in
`foundation-variance-lifetime/spec.md § Requirement: TreeReadExt and TreeNavExt
predicates MUST NOT use over-specified HRTB bounds`.  The D4 (idiom) dimension
of that finding is covered there via the "Rust 1.75+ elision rules make the two
forms equivalent" rationale.

### F20 — check_disposed cfg-gated layout

The primary requirement for the `check_disposed` `#[cfg(debug_assertions)]` /
`#[cfg(not(debug_assertions))]` restructuring (D2 + D4) is specified in
`foundation-concurrency/spec.md § Requirement: check_disposed MUST use explicit
cfg-gated branches`.

### F30 — TreeWriteNav .get() canonical path

The primary requirement for replacing `id.into()` with `id.get()` in
`TreeWriteNav::move_children` and `insert_child` (D4 idiom, flui-tree) is
specified in `tree-soundness-and-idioms/spec.md § Requirement: TreeWriteNav MUST
use Identifier::get()`.
