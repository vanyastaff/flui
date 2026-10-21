# Review instructions

What to look for lives in `AGENTS.md`, which `CLAUDE.md` imports: its "Review guidelines",
"Recurring defects", "Design stance" and "Definition of Done" sections. Read them before the
diff. This file sets severity, evidence and scope for Claude Code Review.

## Evidence

Every finding carries a concrete failure scenario: the input or state, the operation, and the
wrong result (wrong output, panic, hang, leak, lost work, non-finite geometry published). A
claim about behaviour cites the `file:line` that produces it, read in the source rather than
inferred from a name. For a recurring defect, name the case: one failure point alone, two in
competition, or the next operation after containment.

## What Important means here

Important is a defect a user or a later change would hit:

- any "Recurring defects" item with its failure scenario;
- code that disagrees with an accepted ADR in `docs/adr/`;
- `async`, or a lock on per-node state, on the build/layout/paint path;
- a regression test that still passes with the fix reverted, or new behaviour with no test
  through the public API under `tests/`;
- a "Review guidelines" item that leads to wrong behaviour.

Shape, naming and idiom (a transliterated Dart shape, a setter/getter pair, a grab-bag name, a
`SAFETY:` comment that restates the operation) are Nit, each with the better shape. Report at
most five Nits per review.

## Left to the gates

CI already fails on these, so a review spends its findings elsewhere: formatting, clippy
lints (bare `unwrap()` included), spelling, links, workflow lint, and every rule in the
"What the compiler and gates enforce" table of `AGENTS.md`.

These paths get findings only when near-certain and severe:

- `docs/plans/`, archival and kept verbatim;
- files marked `@generated`, and `Cargo.lock`;
- `*.snap` files, checked only against the PR body: a regenerated snapshot the PR doesn't
  explain is a finding, the snapshot contents are not.

## What a review cannot see

A review reads the code and does not run it. Native-platform and GPU changes are run by hand
(CI executes Linux and wasm32 only), so when the PR body shows no run on the affected platform,
say so once in the summary rather than as findings against each line.
