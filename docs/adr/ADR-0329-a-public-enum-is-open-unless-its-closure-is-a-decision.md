# ADR-0329: A public enum is open unless its closure is a decision

- **Status:** Accepted (2026-11-13)
- **Date:** 2026-11-13
- **Related:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §5 (the rule in the
  platform contract, which this record extends to every crate);
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §1 and §3 (what the facade reaches
  carries the Stable promise; `SemanticsRole` and `SemanticsAction` open);
  [ADR-0324](ADR-0324-the-facade-api-is-a-committed-snapshot.md) (the snapshot that writes
  `#[non_exhaustive]` on each line); [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (the ladder)

## Context and problem statement

Adding a variant to a public enum breaks every `match` on it in another crate. Making an exhaustive
enum `#[non_exhaustive]` later breaks the same matches. So the first publication fixes, for every
public enum, whether it can grow. On `main`, `api/flui.txt` lists 271 public enums without
`#[non_exhaustive]` and 413 with it. Nothing decides which is which: the attribute is there where
an author remembered it. In the crates the facade reaches, `#[non_exhaustive]` sits on 8 of
painting's 63 public enums, 6 of objects' 20, 1 of layer's 13 and 9 of engine's 29. Only
`flui-platform` decides each one (`#![deny(clippy::exhaustive_enums)]`, ADR-0151 §5). What makes
the closure of every public enum a decision made before the first publication, and keeps it one?

## Decision drivers

- A variant added in a minor release must not break an application, and a reader of an enum must
  be able to tell whether the set can grow.
- A closed enum is a promise: every `match` on it may be exhaustive, and a new variant is a major
  release.
- Rules are types first and a lint next (ADR-0303): no gate and no allowlist for what a lint states.
- One form of the rule: the platform contract's crate attribute and the rest of the workspace do
  not keep two.
- The departure from Flutter has a Rust reason. Dart has no `non_exhaustive`, so Flutter's enums
  are all closed. In Rust, adding a variant to an exhaustive public enum breaks every downstream
  `match`, so after 1.0 each new variant would need a major release. `#[non_exhaustive]` is
  the language's tool for that kind of semver growth, and an author still writes the same
  `match`, with a `_` arm.

## Considered options

1. `clippy::exhaustive_enums` in `[workspace.lints.clippy]`: every public enum is
   `#[non_exhaustive]`, or carries `#[expect(clippy::exhaustive_enums, reason = "…")]` saying why
   its set cannot grow.
2. `#[non_exhaustive]` on every public enum, no exception.
3. Review decides, with the snapshot's diff as the record.

## Decision

Chosen option: 1, because it is the rule `flui-platform` already holds, stated on the first rung
that can state it, and it keeps closed the enums whose consumers must handle every case.

```rust
#[non_exhaustive]                    // a backend or a later release may add a case
pub enum ImageDecodeError { TooLarge { .. }, Malformed(..) }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[expect(clippy::exhaustive_enums, reason = "a flex lays its children out along one of two axes")]
pub enum Axis { Horizontal, Vertical }
```

### 1. The rule

- `exhaustive_enums = "warn"` is a workspace clippy lint, so it reaches every crate that inherits
  the workspace lints, and CI's `-D warnings` makes it an error. `flui-platform`'s crate-level
  `deny` goes: the rule has one form.
- A public enum is `#[non_exhaustive]` unless its closure is a decision. It is closed only for
  one of three reasons, written in the `reason`:
  - **its set is fixed by what it names**: two axes, a key goes down or up, the four sides of a
    box, a W3C enumeration;
  - **an open end is one of its variants**: a `Custom(…)` or `Other(…)` carries what the set does
    not name (`TransferFormat`);
  - **every consumer must handle a new case**: a new variant is meant to break each `match`,
    because a `_` arm would drop work (`PlatformInput`, an outcome a caller turns into a decision).
- An error enum, an event, a kind reported by a platform or a backend, and a configuration choice
  a later release may extend are open.
- The `reason` names the fact, not the lint: "a flex has two axes", not "closed on purpose".

### 2. Matching an open enum of the workspace

A `match` in another crate of the workspace over a `#[non_exhaustive]` enum names every known
variant in arms of its own and puts `_` in a separate last arm, as the workspace's
`wildcard_enum_match_arm` entry describes. ADR-0089 §3 already does this for `SemanticsRole`.

### 3. No crate-wide exception

Every public enum of the workspace carries its decision on its own declaration. A crate does not
carry `#![expect(clippy::exhaustive_enums)]` at its root, and a new crate starts under the rule.

### Consequences

- Good: whether an enum can grow is on its declaration, and on its line in `api/flui.txt`.
- Good: a closed enum says why, so the next author does not close a set that can grow by copying
  a neighbour.
- Bad: a `match` from another workspace crate over an open enum needs a `_` arm, and loses the
  compiler's check that every variant is named; `wildcard_enum_match_arm` where it is on brings
  that check back as a lint.
- Neutral: a private or `pub(crate)` enum is out of the lint's reach; only the public surface is a
  promise.

### Confirmation

clippy, in CI's `clippy` job with `-D warnings`. Each closed enum's reason is reviewed in the pull
request that adds it, and `api/flui.txt` shows the attribute on each public enum.

## Pros and cons of the options

### 1. The lint in the workspace, with a reason at each closed enum

- Good: the compiler's toolchain states the rule; no gate and no list.
- Good: closed enums stay exhaustively matchable where that is the point.
- Bad: each reason is prose a reviewer judges.

### 2. Every public enum open

- Good: no judgement, nothing to review.
- Bad: `Axis`, `TextDirection` or a key's state would need a `_` arm in every `match`, the arm that
  hides the next variant from the reader; Flutter's closed enums (`Axis`, `TextDirection`) become
  open with nothing gained.

### 3. Review decides

- Good: no lint to satisfy.
- Bad: that is the state on `main`, where 271 enums are closed because nobody decided.
