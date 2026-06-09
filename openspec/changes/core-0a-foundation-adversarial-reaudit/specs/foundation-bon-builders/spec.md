# Foundation bon Builders Specification

## Purpose

Pin the requirement for adopting `bon`-generated named-parameter builders on
multi-positional-argument constructors in `flui-tree`, closing the cycle-3 T-17
deferral.

Owner crates: `crates/flui-tree` (`iter/slot.rs`).

---

## Requirements

### Requirement: Slot::with_siblings MUST be annotated with #[bon::builder] (F14)

`Slot::with_siblings` in `crates/flui-tree/src/iter/slot.rs` MUST be annotated
with `#[bon::builder]`.  `Slot::new` (3 positional parameters: `parent: I`,
`index: usize`, `depth: Depth`) SHOULD also be annotated when the positional-arg
confusion risk is non-trivial.

**Rationale:** `Slot::with_siblings(parent, index, depth, previous_sibling,
next_sibling)` has 5 positional parameters, two of which (`previous_sibling:
Option<I>`, `next_sibling: Option<I>`) are indistinguishable by position — a
silent argument-swap is a valid Rust program.  This is the canonical `bon`
builder case: named-parameter construction prevents positional confusion without
requiring the caller to write a boilerplate builder struct.

The FLUI workspace constitution (Part IV) names `bon` as the canonical builder
dependency.  Cycle-3 deferred `Slot::with_siblings` as T-17 with no completion
trigger; this change closes it.

**`bon` compatibility:** The positional `fn with_siblings(...)` call site MUST be
retained alongside the generated builder (bon's `#[bon::builder]` preserves the
original function signature).

**Acceptance criterion:** SC14 — `grep -n "bon::builder"
crates/flui-tree/src/iter/slot.rs` exits 0.

#### Scenario: Slot::with_siblings has bon builder annotation (SC14)

- GIVEN `crates/flui-tree/src/iter/slot.rs` at HEAD
- WHEN `grep -n "#\[bon::builder\]" crates/flui-tree/src/iter/slot.rs` is run
- THEN it exits with code 0 (at least one match, covering `with_siblings`)

#### Scenario: Builder construction produces an equivalent Slot

- GIVEN a tree node with `parent: I`, `index: 2_usize`, `depth: Depth(3)`,
  `previous_sibling: Some(prev_id)`, `next_sibling: None`
- WHEN `Slot::with_siblings_builder() .parent(p) .index(2) .depth(d)
  .previous_sibling(Some(prev_id)) .next_sibling(None) .call()` is used
  (or equivalent `bon`-generated API)
- THEN the resulting `Slot` is structurally equal to calling the positional
  constructor directly with the same arguments

#### Scenario: Positional constructor is still available

- GIVEN existing call sites that use `Slot::with_siblings(p, i, d, prev, next)`
  positionally
- WHEN `cargo check --workspace --all-targets` is run after adding the annotation
- THEN it exits with code 0 (no existing callers are broken)

#### Scenario: Workspace compiles with the bon annotation

- GIVEN `#[bon::builder]` added to `Slot::with_siblings`
- WHEN `cargo build --workspace` is run
- THEN it exits with code 0
