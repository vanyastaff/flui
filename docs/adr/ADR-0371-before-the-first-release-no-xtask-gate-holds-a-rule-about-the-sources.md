# ADR-0371: Before the first release no xtask gate holds a rule about the sources

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §2 rung 5, §3 and §8 only (a rule about the sources is no longer admitted as an `xtask` gate,
  and `panic-messages` is no longer one). The Confirmation of each record that names a gate §1
  removes, for that gate only: the rule stands and its reader confirms it.
- **Related:** [ADR-0370](ADR-0370-a-pull-request-compiles-for-linux-and-every-other-target-is-mains.md)
  (the pull-request gate this shortens); [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md)
  (the tiers, which stay written in each manifest).

## Context and problem statement

`tools/xtask` held fifteen gates over the workspace's sources, manifests and documents, each a
program with its own self-test and most with an allowlist that only shrinks. They cost more than
the framework at this stage can pay:

| Fact | Value |
|---|---|
| `tools/xtask`, lines of Rust | 54,516 in 92 files; the fifteen gates are 26,600 of them |
| Merged pull requests of 2026-11-16 to 18 that edit `tools/xtask/` | 25 of 121 |
| Allowlists a pull request may have to edit | 9 files, 895 lines |
| Published releases whose users a rule protects | none |

The rules are sound; what is in question is who holds them before the first release, while the
shapes they describe still move. Does a rule about the sources need a program of its own now?

## Decision drivers

- Pre-1.0 a bad shape is fixed, not worked around (AGENTS.md): a rule that erodes costs a
  refactor, not a user.
- A pull request is read in a fresh context before it merges (AGENTS.md, "Before a merge"), from
  the same rules these gates stated.
- One pull request in five paid for the gates' own upkeep.
- A gate removed stays in the history: one revert brings a rule's program back when a release
  needs it.

## Considered options

1. Keep the fifteen gates.
2. Keep the architecture gates (tiers, reach, globals, the panic boundary, `unsafe impl Send`),
   remove the naming and counting ones.
3. Remove all fifteen; the rules stay as text and their reader confirms them.

## Decision

Chosen option: 3, because the split of option 2 keeps the largest programs for the rules a
refactor repairs most cheaply, and no rule here protects a user yet.

### §1. The gates removed

`cargo xtask` has no `workspace`, `reach`, `module-dag`, `globals`, `panic-boundary`,
`send-impls`, `try-new`, `context-names`, `builder-names`, `panic-messages`, `file-length`,
`markers`, `testing`, `docs-paths` or `wgsl` command, and `cargo xtask checks` runs none. Their
allowlists and fixtures are deleted, with the two registries only they read: the `globals` list
and the `[package.metadata.flui.modules]` table of a manifest.

### §2. Where each rule is now

A rule one of them held is stated where it was: AGENTS.md, the ADR that decided it, the crate's
`ARCHITECTURE.md`. A manifest's `tier`, `tier-kind`, `order`, `reach-forbid`,
`allowed-dependents` and its exception lists stay as the statement of the layering. The reader
of "Before a merge" checks a change against these rules; nothing runs them.

A rule that needs a program again takes the first rung of ADR-0303 §2 that states it (a type, a
lint, a tool CI runs, a test of the owning crate). There is no fifth rung until a release makes
one worth its cost, and that is a new record.

### §3. What `xtask` still checks

`cargo xtask checks` keeps the formatters and linters (`cargo fmt`, typos, taplo), the link
check (`docs-links`), the toolchain pin (`toolchain`), the lane's own guard (`paths-filter`), the
changelog fragments (`changelog --check`) and the generated files against their sources
(`font-assets`, `icon-import`, `key-vocabulary`, `recording-vocabulary`, `android-dex`,
`platform-floor`). The API contract jobs (`public-api`, `api-closure`, `send-positions`) are not
part of this record.

### Consequences

- Good, because `tools/xtask` loses about half its code and eight of its nine allowlists.
- Good, because a pull request no longer edits an allowlist to move code a gate counts.
- Bad, because nothing fails when a crate reaches below its tier, a `static` appears, an OS entry
  skips the panic boundary or an `unsafe impl Send` is added: the reader has to see it.
- Bad, because a record's Confirmation that named a gate now names a reader, which
  `docs/adr/README.md` does not count as a confirmation.
- Neutral, because the tiers stay in the manifests with no program reading them: the map is
  documentation, and can be wrong.

### Confirmation

- `the_in_process_checks_include_the_link_check_under_strict` in `checks_contract`
  (`tools/xtask/src/tasks/checks.rs`) lists the checks `cargo xtask checks` runs; a gate added
  back shows there.
- The rules themselves have no confirmation but the reader of "Before a merge" (Consequences).

## Pros and cons of the options

### 1. Keep the fifteen gates

- Good, because every rule fails a pull request that breaks it.
- Bad, because of the upkeep measured above, before any release.

### 2. Keep the architecture gates

- Good, because the rules that are hardest to see in a diff keep their program.
- Bad, because those are the largest gates (the tier, module and panic-boundary gates are 14,000
  of the 26,600 lines), so most of the cost stays.

### 3. Remove all fifteen (chosen)

- Good, because the cost goes and the rules stay written.
- Bad, because of the Consequences above.
