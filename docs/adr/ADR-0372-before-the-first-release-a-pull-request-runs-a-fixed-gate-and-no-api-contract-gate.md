# ADR-0372: Before the first release a pull request runs a fixed gate, and no API contract gate

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0324](ADR-0324-the-facade-api-is-a-committed-snapshot.md) whole (the facade's
  API has no committed snapshot); [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §9 whole (a job only main runs has no narrow form on a pull request);
  [ADR-0370](ADR-0370-a-pull-request-compiles-for-linux-and-every-other-target-is-mains.md) §1
  only for `public-api` and the on-demand jobs, and §3 whole. For the gate each names, in the
  section its own `Superseded-by` line cites, where the rule stands and its reader confirms it:
  ADR-0082, ADR-0089, ADR-0098, ADR-0151, ADR-0266, ADR-0280, ADR-0304, ADR-0305, ADR-0311,
  ADR-0312, ADR-0322, ADR-0323, ADR-0327, ADR-0329, ADR-0330, ADR-0331, ADR-0368.
- **Related:** [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  (the source gates, removed by the same reasoning).

## Context and problem statement

After ADR-0370 a pull request that compiled anything ran `clippy`, `test` and `public-api`, plus
seven jobs main runs, each when `plan` found its inputs in the diff: `test-trybuild`, `size`,
`feature-scope`, `api-contracts` (`send-positions`), `miri`, `perf-gates`, `test-without-agent`.
Main ran two more API contract gates, `api-closure` and `send-positions`, in `api-contracts`.

| Fact | Value |
|---|---|
| `public-api` on a pull request | 9 to 15 minutes, the gate's longest job after `test` |
| Pull-request runs of 2026-11-18 in which an on-demand job or `public-api` failed alone | 0 of 61 |
| `api-closure` on main | the failing job of 7 of the 8 red runs on main from 2026-11-17 22:37 to 2026-11-18 06:31 |
| Code that decides and checks the on-demand jobs, and the three API gates | 6,469 lines of `tools/xtask`, and the 36,852-line snapshot `api/flui.txt` |
| Toolchains a pull request installs for `public-api` | a nightly beside the pinned stable |

No published release has users whose code an API change breaks. Which checks does a pull request
need while the facade's surface still changes every day?

## Decision drivers

- A two-line change waits as long as a rewrite while the gate depends on what the diff touches.
- An API contract protects the users of a published release; before the first one there are
  none, and the surface is meant to move (AGENTS.md: a bad shape is fixed, not worked around).
- A job that failed on a pull request only beside `clippy` or `test` adds minutes, not findings.
- The decision is reversible: one revert brings back a gate and its job.

## Considered options

1. Keep the three API gates and the on-demand jobs (ADR-0370 as it stood).
2. Keep `public-api` on every pull request, drop the on-demand jobs.
3. A fixed pull-request gate (`clippy`, `test`); every other job on main; no API contract gate.

## Decision

Chosen option: 3, because it makes the gate the same for every pull request and removes the jobs
that found nothing `clippy` and `test` did not.

### §1. The pull-request gate is fixed

A pull request that compiles anything runs `checks`, `plan`, `deps`, `clippy` and `test`
(`HEAVY_JOBS` is `clippy` and `test`). `plan` decides the lane alone: it has no output that asks
for a job. `test-trybuild`, `size`, `miri`, `perf-gates` and `test-without-agent` are `FULL_JOBS`,
run by main, the merge queue and a manual run. `feature-scope` is removed; main's
`feature-matrix` runs every package's features, and `cargo xtask feature-matrix --packages` runs
a crate's features locally.

### §2. No API contract gate

`cargo xtask` has no `public-api`, `api-closure` or `send-positions` command, CI no
`public-api` or `api-contracts` job, and the repository no `api/flui.txt`, no
`[package.metadata.flui.api-closure]` table and no `tools/xtask/allowlists/`. `cargo xtask
expectations` no longer counts an API snapshot or an allowlist. The rules those gates held stay
written where they were decided (ADR-0089 §2 and §6, ADR-0151 §8, ADR-0323 §2 and §3); the
reader of "Before a merge" checks a change against them.

### Consequences

- Good, because every pull request runs the same five jobs, none on a nightly toolchain.
- Good, because main loses a gate that kept it red for hours on findings no pull request saw.
- Bad, because a change to the facade's surface shows only in the code diff: no snapshot lists
  what it adds or removes, and the reader has to see it.
- Bad, because a public signature can name an upstream type ADR-0089 §2 forbids, or a new
  `Send`/`Sync` position can appear, and nothing fails.
- Bad, because a trybuild expectation, a size, a miri finding or a perf count a pull request moves
  fails main after the merge, as `feature-matrix` and `doc-test` already did.
- Neutral, because the first release brings the question back: a published API needs a contract,
  and that is a new record.

### Confirmation

- `heavy_jobs_list_matches_the_jobs_gated_on_heavy` and `lane_lists_match_the_job_conditions` in
  `aggregator_contract` (`tools/xtask/src/change_scope/aggregator.rs`) pin `HEAVY_JOBS` and
  `FULL_JOBS` to the jobs whose `if:` names their lanes, and `cargo xtask ci-verify` fails a run in
  which a job of another lane ran.
- The API rules have no confirmation but the reader of "Before a merge" (Consequences).

## Pros and cons of the options

### 1. Keep the API gates and the on-demand jobs

- Good, because an API change carries its diff and a forbidden upstream type fails.
- Bad, because of the measurements above, before any release.

### 2. Keep `public-api`, drop the on-demand jobs

- Good, because an API change still carries its diff.
- Bad, because `public-api` stays the gate's second longest job, on a nightly toolchain, and
  reviews a surface no user depends on yet.

### 3. A fixed gate, no API contract gate (chosen)

- Good, because the gate is short and the same for every pull request.
- Bad, because of the Consequences above.
