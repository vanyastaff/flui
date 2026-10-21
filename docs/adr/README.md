# Architecture decision records

An ADR records one cross-crate decision the code follows, with the options that lost and why. The
set in this directory is the memory of the project: a reader who was not there learns from it what
was decided, under which forces, and how to tell that the code still complies.

The format is [MADR 4.0](https://adr.github.io/madr/) with the header this repository has always
used. MADR is the template the [ADR organisation](https://adr.github.io/) recommends; its sections
force the two things a decision is most often missing: the drivers that decided it, and the pros
and cons of every option, not only the chosen one.

## When to write one

Write an ADR when all three hold:

1. **Hard to reverse.** Changing the decision later costs more than a refactor: a crate boundary, a
   public contract, a thread model, a file format, a gate every pull request must pass.
2. **Surprising without context.** A reader of the code would ask "why this way?".
3. **A real trade-off.** There were alternatives, and one was chosen for stated reasons.

A choice that fails one of the three is a code comment, a crate's `ARCHITECTURE.md` entry or a
line in a spec's `design.md`.

A feature's design lives in `docs/plans/specs/<feature>/design.md` while the feature is in work:
that document is the request for comments, with the research, the options and the decisions. Once
the feature lands, the decisions that outlive it move into ADRs and the spec is deleted
(`AGENTS.md`, "Working here").

## Template

Copy this file's structure; delete the comments. English, present tense, no process markers (no
pull-request numbers, phases, cycles or task IDs: state the invariant, not the history).

```markdown
# ADR-NNNN: <the decision in one sentence, as a statement>

- **Status:** Proposed | Accepted (YYYY-MM-DD) | Deprecated (YYYY-MM-DD)
- **Date:** YYYY-MM-DD
- **Supersedes:** [ADR-MMMM](ADR-MMMM-slug.md) §N only (what exactly, in one clause)
- **Superseded-by:** [ADR-PPPP](ADR-PPPP-slug.md) for §N only (added to this file later)
- **Related:** [ADR-QQQQ](ADR-QQQQ-slug.md) (why it is related, in a few words)

## Context and problem statement

<!-- Two to six sentences, or a short table of measured facts with their evidence (path:line at
a named commit, an issue, a run). Make the scope explicit: which crates, which contract. End with
the question the decision answers. -->

## Decision drivers

- <!-- a quality, a constraint or a force: "only flui-app may depend on the backend",
     "the frame path is synchronous", "CI executes Linux only" -->
- …

## Considered options

1. <!-- option title -->
2. …

## Decision

<!-- "Chosen option: 1, because …" When an ADR must carry several decisions (a boundary and the
items on each side of it), number them §1, §2, … so other ADRs can supersede one of them. Each
decision names the Rust shape it mandates: the type, trait, bound, error enum or gate, not a
convention a reviewer has to remember. -->

### Consequences

- Good, because …
- Bad, because …
- Neutral, because …

### Confirmation

<!-- How compliance is checked, in this order of preference: the compiler (a bound, a sealed
trait, a compile-fail fixture), a clippy lint, a `cargo xtask` gate, a test named here, an executed
run on the environment (`docs/platform-parity.md` names it). "Review" alone is not a confirmation. -->

## Pros and cons of the options

### 1. <!-- option title -->

- Good, because …
- Bad, because …

### 2. …

## More information

<!-- Primary sources with pins: `owner/repo@sha path:line`, an issue URL, a vendor document URL,
a dated run. Research notes are not kept in the repository; cite what they rest on. When the
decision should be revisited, say what would trigger it. -->
```

## Rules

- **One decision per ADR** where the decision stands alone. A boundary ADR may hold the numbered
  decisions that define both sides of the boundary; a later ADR supersedes one section, not the
  file.
- **Status is a fact about the code.** `Proposed` until the owner accepts it; `Accepted (date)`
  once the decision is the rule, with a "Not implemented: …" clause while the code catches up;
  code that disagrees with an accepted ADR is a defect in one of them (`AGENTS.md`, "ADR Policy").
  `Deprecated` when the decision is withdrawn without a replacement. A replaced decision gets
  `Superseded-by:` in its header and keeps its text: git keeps history, the ADR keeps the reasoning.
- **Supersede explicitly and narrowly.** Name the section and the clause. "Supersedes ADR-0082"
  without a section retires decisions the author never read.
- **Rust shapes, not conventions.** A decision that can be a type, a bound, a sealed trait, an
  error enum or a gate is written as that; a decision that can only be a convention says which
  check (lint, `xtask` gate, test) makes the convention fail loudly.
- **Failure paths are part of the decision.** Where user code runs, where a hook can be missing,
  replaced or panic, where arithmetic can overflow: the ADR says what happens (`AGENTS.md`,
  "Recurring defects").
- **Sources are primary.** Another framework's behaviour is cited at its repository and commit, an
  issue or its documentation; "Flutter does it this way" without a pin is not a reason, and matching
  another framework is never a reason by itself (ADR-0027).
- **Tests are references.** An ADR that names a test pins its name; `rg` it before renaming the
  test.
- **Numbering and names.** The next free number, four digits; the filename is the number and a
  kebab-case slug of the title: `ADR-0254-window-contract.md`. Never renumber or rename: other
  documents link the file.

## Reading order

`docs/architecture.md` describes the code as it is and links the ADRs that shaped each layer.
`docs/platform-parity.md` and `docs/gaps.md` hold status; an ADR never does.
