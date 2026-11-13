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
- a "Review guidelines" item that leads to wrong behaviour;
- new code that uses a form from the "Not" column of `AGENTS.md`'s "Current forms", or a
  replacement or reversal whose old form still has users or records that teach it.

Shape, naming and idiom (a transliterated Dart shape, a setter/getter pair, a grab-bag name, a
`SAFETY:` comment that restates the operation) are Nit, each with the better shape. Report at
most five Nits per review.

## Left to the gates

CI already fails on these, so a review spends its findings elsewhere: formatting, clippy
lints (bare `unwrap()` included), spelling, links, workflow lint, and every rule in the
"What the compiler and gates enforce" table of `AGENTS.md`.

These paths get findings only when near-certain and severe:

- `docs/plans/specs/`, the live specs, whose task IDs and status tables are their purpose: a
  finding there is a spec that contradicts an accepted ADR or a status the diff makes false;
- files marked `@generated`, and `Cargo.lock`;
- expectation files (`*.snap`, `*.stderr`, `api/flui.txt`, perf baselines, allowlists), checked
  only against the `## Expectations` section of the PR body (ADR-0331): an edit whose line there
  does not say why the old expectation was wrong is a finding, the file's contents are not.

## What a review cannot see

A review reads the code and does not run it. Native-platform and GPU changes are run by hand
(CI executes Linux and wasm32 only) from the owner's device list, so a missing run on the
affected platform is not a finding and does not go in the summary either; check only that the
PR body names the platforms the change still needs a run on.
