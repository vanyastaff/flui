# ADR-0370: A pull request compiles for Linux, and every other target is main's

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §9 only, for five jobs: `test-nested`, `test-features`, `doc`, `wasm-check` and `cross-typecheck`
  run on main with no narrow form on a pull request. Unchanged: the `ON_DEMAND` rows and how
  `plan` decides them.
- **Related:** [ADR-0324](ADR-0324-the-facade-api-is-a-committed-snapshot.md) (`public-api` stays
  in the pull-request gate, as its §3 decides).

## Context and problem statement

A pull request that compiled anything ran every Linux job, wasm32 and four platform typechecks
(`HEAVY_JOBS`), on the measurement that hosted runners did not queue for this repository. They do
now. Over the 61 completed pull-request runs of `ci.yml` on 2026-11-18, 01:56 to 07:42 UTC:

| Fact | Value |
|---|---|
| A green run | 21 jobs, 191 job-minutes, 38 minutes from start to `ci` (58 at most) |
| Time its jobs spent waiting for a runner | 108 minutes summed over the jobs |
| Job-minutes spent on runs later cancelled or failed | 51% (4037 of 7852) |
| `doc`, `test-features`: runs they failed alone | 0 of 51 each |
| `test-nested`: runs it failed alone | 1 of 51 |
| `wasm-check`, `cross-typecheck`: runs only they failed | 10 of 61 |

The repository merges tens of pull requests a day on one account's runners, two of the platform
legs need a macOS runner, and a two-line change waited as long as a rewrite. Which jobs does a pull
request have to pass before it merges?

## Decision drivers

- The wait for a pull request is what limits the work: a run has to end in minutes.
- One account's runner allowance serves every pull request and every push to main.
- Before the first release no platform other than Linux is run by CI at all: Windows, macOS, iOS
  and Android are type-checked there and run on devices and in `nightly-platforms.yml`.
- A red main has to stay visible (the "CI is red on main" issue), because more checks now reach a
  change only after its merge.

## Considered options

1. Every Linux job, wasm32 and the platform typechecks on every pull request (the state before).
2. A narrow form for each of them, decided by the diff (ADR-0303 §9).
3. A Linux gate on the pull request; the other jobs on main.
4. Nothing compiled for a pull request; the suites on a schedule.

## Decision

Chosen option: 3, because it removes the wait and the queue where the measurements put them, and
keeps the one platform CI can execute in front of every merge.

### §1. The pull-request gate

A pull request that compiles anything (lane `wide`) runs `checks`, `plan`, `deps` and
`HEAVY_JOBS`, which is `clippy`, `test` and `public-api`, then the `ON_DEMAND` jobs its diff asks
for. The three `HEAVY_JOBS` wait for `plan` alone, not for `checks`. In `test` the build that links
the examples and benches is a `full`-lane step: `clippy --all-targets` type-checks the same
targets on the pull request.

### §2. What main adds

Lane `full` (a push to main, the merge queue, a manual run) adds `FULL_JOBS`: `feature-matrix`,
`doc-test`, `test-nested`, `test-features`, `doc`, `wasm-check` and `cross-typecheck`. No job
compiles for wasm32, Windows, macOS, iOS or Android on a pull request. A change that breaks one
of them is found by main's run and fixed forward or reverted, as AGENTS.md says of a red main.

### §3. A job added later

A job added to `FULL_JOBS` after this record still gets its `ON_DEMAND` row when a pull request
can break it (ADR-0303 §9). The five jobs above are the exception this record makes, and a narrow
form for one of them is a new row, not a return to `HEAVY_JOBS`.

### Consequences

- Good, because a pull request runs 7 jobs instead of 21 and no macOS runner, about 45
  job-minutes by the same runs' averages.
- Good, because main's run is unchanged: every job still runs on every push.
- Bad, because a pull request can merge green and break the wasm32 or a native build, a nested
  suite, a feature-gated test or a rustdoc link. In the sample one run in six carried a break
  only `wasm-check` or `cross-typecheck` saw.
- Bad, because the author of that break learns of it from main, after the merge, and the next
  author works on a red main until it is fixed.
- Neutral, because `public-api` (13 minutes by the same averages) is now as long as `test`, the
  condition under which ADR-0324 §3 moves it to an `ON_DEMAND` row.

### Confirmation

- `heavy_jobs_list_matches_the_jobs_gated_on_heavy` and `lane_lists_match_the_job_conditions` in
  `aggregator_contract` (`tools/xtask/src/change_scope/aggregator.rs`) pin `HEAVY_JOBS` and
  `FULL_JOBS` to the jobs whose `if:` names their lanes.
- `cargo xtask ci-verify`, the `ci` job, fails a run in which a job of another lane ran or a
  planned one skipped.

## Pros and cons of the options

### 1. Everything on every pull request

- Good, because a platform or rustdoc break is found before the merge.
- Bad, because of the measurements above: 38 minutes for a green run, half the job-minutes spent
  on runs nobody used.

### 2. A narrow form for each job

- Good, because it is the rule ADR-0303 §9 already states.
- Bad, because `wasm-check` and `doc` read every crate, so their narrow form is nearly every pull
  request, and close to half of the merged pull requests touch the platform crates that
  `cross-typecheck` reads.

### 3. A Linux gate, the rest on main (chosen)

- Good, because the gate is the three jobs that fail for a pull request's own code on the
  platform CI executes.
- Bad, because of the breaks that reach main (Consequences).

### 4. Nothing compiled for a pull request

- Good, because a pull request merges at once.
- Bad, because a pull request merges as soon as its checks are green here, tens of times a day,
  and the Linux suite is the only thing between two scheduled runs.
