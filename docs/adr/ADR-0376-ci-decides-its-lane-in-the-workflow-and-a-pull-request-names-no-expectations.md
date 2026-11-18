# ADR-0376: CI decides its lane in the workflow, and a pull request names no expectations

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0331](ADR-0331-a-pull-request-names-every-expectation-it-edits.md) whole (no
  check reads a pull request's body; the rule it held stays as a review question);
  [ADR-0370](ADR-0370-a-pull-request-compiles-for-linux-and-every-other-target-is-mains.md) §2 only
  for `xtask-native.yml` on a pull request, and its Confirmation;
  [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  its Confirmation (the `ci` job's rule is a shell step, not `cargo xtask ci-verify`);
  [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §1 only for the CI lanes and change scope it lists as xtask's.
- **Related:** [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  and ADR-0375 (the same reduction of `tools/xtask` before the first release).

## Context and problem statement

After ADR-0375 a pull request's gate was fixed, yet `tools/xtask` still decided it: `cargo xtask
affected` classified the changed files into four lanes, `cargo xtask ci-verify` recomputed which
jobs each lane runs and failed a run whose skips differed, and `cargo xtask expectations` held
every pull request's body to an `## Expectations` section. Two more workflows ran on pull
requests:

| Fact | Value |
|---|---|
| `tools/xtask/src/change_scope/` (lanes, aggregator, expectations) | 3,931 lines |
| `xtask-native.yml` on 2026-11-18, on Windows and macOS runners | 52 runs, 422 minutes |
| `fuzz.yml` on 2026-11-18 | 17 runs, 243 minutes |
| `expectations.yml` on 2026-11-18 | 75 runs, 115 minutes, and a section in every body |

The owner wants `tools/xtask` gone before the first release and CI made of the tools every Rust
project runs. What does CI need from xtask to pick a lane and judge a run?

## Decision drivers

- The rule a gate protects stays written; a program holds it only when no reader can.
- A workflow step everyone can read beats a program only its authors know.
- Repository tooling does not need a Windows and a macOS runner on every change to it.

## Considered options

1. Keep `affected`, `ci-verify`, `expectations` and the two workflows on pull requests.
2. Decide the lane and judge the run in shell inside `ci.yml`; drop the body check; run
   `xtask-native.yml` and `fuzz.yml` on a schedule.
3. Drop the lanes too: every pull request runs the whole gate.

## Decision

Chosen option: 2, because it removes the program and the runner minutes while keeping the
documentation lane, which most documentation pull requests take.

### §1. The lane is a shell step

`plan` lists the pull request's changed files, a rename by both its paths, and takes `docs` when
every one is documentation (a root `*.md` or licence text, `docs/` but `docs/gaps.md` and
`docs/platform-parity.md`, `changelog.d/`, a crate's own `ARCHITECTURE.md`, `CHANGELOG.md` or
licence text, the issue templates, `CODEOWNERS`), `wide` otherwise, and `full` for every other
event. `book/` is not documentation: its chapters are doctests of the facade (`src/book.rs`). A
listing that fails stops the step; it never leaves a lane. There is no `tooling` lane: a change to
repository tooling runs the gate.

### §2. The `ci` job is a shell step

`ci` fails when a job it needs failed or was cancelled, when a job its lane runs (`docs`: `checks`,
`plan`; `wide`: and `deps`, `clippy`, `test`; `full`: every job it needs) did not succeed, and when
a run on main, the merge queue or a manual run takes another lane than `full`. A job left out of
its `needs` gates nothing.

### §3. No check reads a pull request's body

`cargo xtask expectations`, `expectations.yml` and the template's `## Expectations` section are
removed. AGENTS.md "Before each push" item 14 keeps the question: an expectation moved to turn a
check green is the defect the check was there to find, and the body says why each changed.

### §4. Two workflows leave pull requests

`xtask-native.yml` runs weekly and by hand; `fuzz.yml` every night and by hand. `deps`' advisories
step blocks on main and reports on a pull request.

### Consequences

- Good, because `tools/xtask` loses `affected`, `ci-verify`, `expectations` and `paths-filter`,
  and a pull request no longer starts a Windows or macOS runner for xtask's own tests.
- Bad, because a documentation file a crate starts to include with `include_str!` lets a docs-only
  pull request skip the jobs that would read it; main's run finds it.
- Bad, because a pull request that brings a RustSec advisory, or breaks a fuzz target or xtask on
  Windows or macOS, turns main red, or the next scheduled run, instead of failing itself.
- Bad, because a regenerated snapshot or a removed test is justified only if the author writes why
  and the reader asks.

### Confirmation

- `actionlint` (in `checks`) parses both shell steps' workflow; a lane's run is judged by the
  `ci` job itself on every run.
- `ci_lets_only_the_advisories_pass_on_a_pull_request` (`tools/xtask/src/tasks/deps.rs`) pins the
  advisory step's condition.

## Pros and cons of the options

### 1. Keep the program and the workflows

- Good, because a skip that drifts from the plan fails, and every edited expectation is named.
- Bad, because of the lines and minutes above, and every body carries a section.

### 2. Shell steps, no body check, scheduled side workflows (chosen)

- Good, because what decides a run is a dozen lines of the workflow.
- Bad, because of the Consequences above.

### 3. No lanes

- Good, because there is nothing to decide.
- Bad, because a documentation pull request would wait about twenty minutes for a gate it cannot
  break.
