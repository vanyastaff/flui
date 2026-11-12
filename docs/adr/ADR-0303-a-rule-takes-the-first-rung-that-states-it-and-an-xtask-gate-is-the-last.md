# ADR-0303: A rule takes the first rung that states it, and an xtask gate is the last

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes in part:** [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) §4, the last
  clause of condition 3: a gate's self-test runs in `cargo test -p xtask`, not in
  `cargo xtask checks` (§4). Unchanged: conditions 1, 2 and 4, and the self-test itself.
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) Confirmation, the last
  bullet: the hand-written `catch_unwind` calls are a `clippy.toml` entry with an expectation at
  each site, not a counting gate (§6).
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (the tier gate, a rule only
  a gate can hold); [ADR-0097](ADR-0097-no-process-global-state-gate.md) (an allowlist entry that
  names its grant); [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §16 (the OS entry
  boundary); [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md) (`inspect`'s `Debug` escape
  hatch); [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (the
  `clippy.toml` entries that gate OS versions)

## Context and problem statement

`tools/xtask` is the repository's own automation crate. Since ADR-0078 §4 let scans return under
four conditions, nearly every new repository rule has arrived as a new `xtask` gate:

| Measure (`tools/xtask`) | 2026-09-24 | 2026-11-03 |
|---|---|---|
| Lines of Rust | 13.0k | 45.2k |
| `cargo xtask` commands | 35 | 57 |
| Lines of allowlist (`tools/xtask/allowlists/`) | 0 | 1334, in 9 files |
| Share of the code that is source-rule gates | — | about 64% |

The cost is not CI time: every check of `cargo xtask checks` together takes about 30 seconds.
The cost is the code. `tools/xtask` is now larger than `flui-painting` or `flui-foundation`, and
each gate brings its own parser of the source, its own allowlist format and its own self-test.

A gate is the change one contributor or agent can finish alone: it touches no crate, needs no
type decided and absorbs the existing sites into an allowlist. The four conditions of ADR-0078 §4
say when a scan is admissible, not whether a gate is the right place for the rule, and condition 1
("no type or stock lint can state the rule") is a judgement nobody writes down. Two recent gates
count sites of rules a stock mechanism states: hand-written `catch_unwind` calls
(clippy's `disallowed-methods`) and uses of `inspect`'s `Value::Debug` escape hatch
(`#[deprecated]`). The self-tests of the gates run twice in CI: in `cargo xtask checks` and, for
about half of them, again in `cargo test -p xtask`.

What belongs in `xtask`, where does a new rule go instead, and how is an exception to a rule
written?

## Decision drivers

- Rules are types (AGENTS.md): the compiler first, a clippy lint next, a grep last.
- An exception sits where it applies, and the build fails once it no longer applies.
- `xtask` is code the project maintains: a gate has to pay for itself against the rung it
  displaces.
- The standard commands find what they can: `cargo clippy` and `cargo test` run a rule without a
  project-specific command.
- One record decides where a new check goes, so a contributor or an agent does not have to ask.
- No nightly toolchain for lints and no workspace crate that exists only to host tooling
  (ADR-0078's rejection of a custom lint).

## Considered options

1. ADR-0078 §4's four conditions as the only bar (status quo).
2. A ladder of places, a written admission for each gate, and exceptions as compiler-checked
   expectations at the site.
3. Custom lints (dylint) for the source rules.
4. A pattern tool (ast-grep) as the default for source rules.
5. Every rule as a test in the crate it guards.

## Decision

Chosen option: 2, because it keeps the gates that only `xtask` can hold, moves every rule a
stock lint can state to that lint, and puts each exception where the compiler sees it.

### §1. What `xtask` holds

`xtask` holds four kinds of command and nothing else:

- **Orchestration with logic of its own.** The CI lanes and change scope (`plan`, `affected`,
  `ci-verify`, `paths-filter`), the command lines that must match between a contributor and CI and
  that choose packages, features, targets or environments (`test`, `clippy`, `cross-typecheck`,
  `feature-matrix`, `miri`, the `wasm-*` steps), and the `checks` bundle.
- **Generators and importers whose output is committed** (`key-vocabulary`, `icon-import`,
  `font-assets`, `changelog`, …).
- **Tools for working in the repository** (`worktree`, `doctor`, `perf`).
- **Gates admitted under §3.**

A new step that is one tool call with fixed arguments is written in the workflow and in
`docs/testing.md`, not wrapped in a command.

### §2. The ladder

A new rule takes the first rung that can state it:

1. **A type**: a signature, a bound, a sealed trait, a compile-fail fixture.
2. **A rustc or clippy lint**: a level in `[workspace.lints]`, an entry in `clippy.toml`
   (`disallowed-methods`, `disallowed-types`, `disallowed-macros`) whose `reason` names the ADR and
   the replacement, or `#[deprecated]` on the item being retired.
3. **A tool CI already runs**: cargo-deny (bans, licenses, advisories, duplicate versions), typos,
   taplo, lychee. Adding a new tool is a decision of its own, because every contributor installs
   it.
4. **A test of the crate that owns the rule**: a generated file against its source, a table
   against its enum, a rule over one crate's own modules. It is reached by `cargo test -p <crate>`
   and rewrites its expected file under an environment variable, as `TRYBUILD=overwrite` does.
5. **An `xtask` gate**, admitted under §3.

A rule may use two rungs: a type for the shape and a test for its table.

### §3. Admitting a gate

A new gate meets ADR-0078 §4's conditions 1, 2 and 4, condition 3 as §4 amends it, and two more:

- **It reads what no crate's build sees**: the package graph, the manifests, the workflows, the
  documentation tree, or a rule about every crate's text at once.
- **Its ADR rules out each rung above it**: one line per rung of §2 saying why that rung cannot
  state the rule. A gate without those lines is not admitted.

### §4. Self-tests are tests

A gate's self-test runs in `cargo test -p xtask`: the table test `checks_contract` runs every
self-test (`every_self_test_reports_exactly_its_planted_findings`) and fails when a command that
accepts `--self-test` is missing from its list (`every_command_with_a_self_test_is_listed`).
`cargo xtask checks` runs the gates, not their self-tests. The `--self-test` flag stays as the way
to run one self-test from the command line; a self-test that needs more than `xtask` (the one of
`api-closure` reads the workspace's nightly rustdoc JSON) runs in the job that runs its gate.

### §5. An exception is an expectation at the site

A site exempt from a lint carries `#[expect(<lint>, reason = "…")]` on the narrowest item that
holds it: the `let` or expression statement, otherwise the function. A macro invocation is not
such a statement: rustc does not carry an attribute on it into the expansion, so the value is
bound by a `let` first.

The attribute is `expect`, not `allow`: an expectation that no longer suppresses anything raises
`unfulfilled_lint_expectations`, which `-D warnings` makes an error. An exception therefore cannot
outlive its site, the property ADR-0078 §4 condition 4 asks of an allowlist. It is not the inline
marker that condition rules out: a marker is a comment a scan trusts, and an expectation is
checked by the compiler.

No expectation of `clippy::disallowed_methods`, `disallowed_types` or `disallowed_macros` covers a
whole file or module: one lint carries every entry of `clippy.toml`, so a wide expectation would
also hide the other entries in that scope (ADR-0244's version checks).

The entries of `clippy.toml` are production rules, as `allow-unwrap-in-tests` makes
`clippy::unwrap_used` one. Test code is exempt through two attributes per crate, never per site:
`#![cfg_attr(test, allow(clippy::disallowed_methods, reason = "…"))]` on the crate root, which
covers its `#[cfg(test)]` modules, and `#![allow(clippy::disallowed_methods, reason = "…")]` on
`tests/main.rs`. The crate's non-test build lints the same production code without the allowance,
so the exemption never reaches it.

### §6. Catching a panic by hand is a disallowed method

`clippy.toml` lists `std::panic::catch_unwind` under `disallowed-methods`, with the reason: run
user code through `flui-foundation`'s commit-then-call primitives (ADR-0287), and an OS entry
through `owner::panic_boundary::os_entry` (ADR-0151 §16).

- The primitives themselves (`crates/flui-foundation/src/panic.rs` and its submodules,
  `flui-native`'s `owner::panic_boundary`) carry an expectation at each of their catch sites,
  with a reason that names them as the boundary.
- Every other production site carries
  `#[expect(clippy::disallowed_methods, reason = "ADR-0287: <why this site catches by hand>")]`.
  A site that moves to a primitive deletes its call, and its expectation becomes an error until it
  is removed too. The expectations with that reason are the migration's remaining count.
- A new hand-written site needs a new expectation, which the diff shows and review weighs, as it
  weighed a new allowlist entry before.

### §7. The `inspect` escape hatch is deprecated at its constructor

`Value::Debug` of `flui_foundation::inspect` holds an `Untyped`, whose one public constructor,
`Untyped::new`, is `#[deprecated]` with a note that names the typed values of ADR-0242. Each
remaining hand-written site carries `#[expect(deprecated, reason = "…")]`. A field of a
`#[derive(Inspect)]` type opts in with `#[inspect(debug)]`, which is that field's written
exception; the derive allows the constructor in the code it generates. The variant stays
matchable without a warning, so code that reads a `Value` is unaffected, and the gate that counted
`Value::Debug(` sites with its allowlist is deleted.

### §8. `panic-messages` stays a gate

The rule that a production `expect` or `panic!` names its invariant (`BUG: …`) or its `try_` twin
is about the text of a string literal, in every crate. It passes §3:

- **Type**: a typed replacement would make bare `expect` and `panic!` disallowed and put an
  extension method and a macro in their place, rewriting about 4,600 `.expect(` calls to restate a
  convention their message already carries.
- **Lint**: no lint reads message text; `clippy::expect_used` and `clippy::panic` forbid the call,
  not its words.
- **Tool**: no tool CI runs reads Rust string arguments by position.
- **Owning crate**: the rule has no owner; it holds in every crate.

### §9. A check only main runs gets its pull request when the diff reaches its inputs

A job too slow for every pull request (FULL_JOBS) still holds a rule each pull request can break.
When a pull request cannot see the break, `main` goes red one merge later, and the next author
pays for it. So a check that runs only on main has a narrow form that a wide-lane pull request
runs when its diff reaches what the check reads. The narrow form is worked out by
`cargo xtask affected` in the `plan` job, the tool that already picks the lane. It is not a gate
of its own.

- **What the check reads comes from its own expectations.** A trybuild expectation quotes, in its
  `--> path` and `::: path` lines, every file of the error chain: the fixture, and the declaration
  of each type or item the compiler names. `plan` reads the tracked `*.stderr` files on each run
  (`change_scope::diagnostics`) and sets `trybuild_required` when the diff touches one of these:
  - a cited file;
  - a file in a case directory: beside an expectation, or in a directory a runner's `pass` or
    `compile_fail` path names (a passing case has no expectation and runs its assertions);
  - a runner of the suites;
  - the source of a workspace procedural macro, whose diagnostics point at the invocation and
    are copied into the expectations;
  - a changed line, added or removed, in a crate a suite compiles, that writes a message the
    compiler copies with no location: a line of a diagnostic attribute
    (`#[diagnostic::on_unimplemented]`, `#[deprecated]`, `#[must_use = …]`, a `message`,
    `label` or `note` key), a `compile_error!`, or a run of 20 or more characters of prose that
    an expectation's message lines quote (a proc macro's or a panic's text);
  - the toolchain file, `Cargo.lock` or the nextest groups.

  No list of these files is stored, so there is none to keep in step with the expectations.

  What these inputs miss is a declaration a case depends on that its output does not name. A
  passing case depends on the public API of every crate it uses. A compile-fail case can depend
  on a field's privacy or a missing bound. For example, making a private field `pub` lets its
  privacy case compile, and the expectation cites only the fixture. A change of that kind still
  turns main red, where `test-trybuild` runs on every merge and the break is fixed forward.
  Closing the gap would make every source of a crate a suite compiles an input, and the suites
  would then run on most pull requests. The inputs above cover the drift main has shown: a type
  an expectation names, and a message copied into one.
- **The narrow form runs over the changed crates.** `feature-scope` runs feature-matrix's
  `cargo hack clippy --each-feature` over the changed crates that have features, and over every
  crate whose manifest changed (`hack_args`). It does this in a pull request's full mode too, when
  a `Cargo.lock` or workflow change sits beside a crate's change. It does not cover a dependent's
  feature-gated edge, which main's feature-matrix still compiles: those edges would double the
  pass for a change to one widely used crate.
- **What the gate reports is read from its list and the facade's crates.** `send-positions` lists
  every `Send` or `Sync` an author wrote in the facade's public API (ADR-0136 §3). In a crate the
  facade reaches, `plan` sets `send_positions_required` when a changed library line, added or
  removed, does one of these (`change_scope::thread_positions`):
  - names `Send` or `Sync` as a word, outside a comment;
  - declares a `pub use` or `pub mod`, or sits inside a `pub use` that spans several lines (read
    from the blob on its side of the diff), which decides what the facade re-exports;
  - names, in the file that defines an item the list names, a segment of that item's path
    there (`AppConfig`, `memory_reclaim`), so a rename or a move on a line apart from the
    bound is seen. An implementation's entry (`impl Trait for Type`) resolves each path it
    holds.

  A change to the list, the gate's rule or the rustdoc JSON loader it shares with `api-closure`
  sets it too: their self-tests read built-in documents and cannot see the workspace's report.

  A pull request then runs `api-contracts` with `send-positions` only; `api-closure` stays on
  main. What these lines miss is an item that turns public on a line that names no marker, while
  its bound sits on another line. Main's run still catches that.
- **The aggregator recomputes the run.** Each such job is a row of `ON_DEMAND`
  (`change_scope/aggregator.rs`), which names the job and the `plan` output that asks for it.
  `ci-verify` reads that output and fails a job that ran without being asked, or skipped when it
  was asked, the same way it does for the lanes (rule 2).
- **A pull request that does not reach the inputs pays nothing.** The job skips, and its only
  cost is the `plan` step's read of the expectations, done without compiling.

### Consequences

- Good, because a rule a stock lint states is checked by `cargo clippy` in every editor and every
  job that already runs it, with no parser of our own.
- Good, because an exception sits beside the code it excuses and fails the build once the code is
  gone, instead of a count in a file elsewhere.
- Good, because `cargo xtask checks` runs each gate once, and `cargo test -p xtask` holds every
  self-test with a test that notices a missing one.
- Good, because the place of a new rule is decided by the ladder, and the reason a gate was chosen
  is written in its ADR.
- Bad, because a lint sees only the code a build compiles: a `cfg` that no CI job compiles is not
  checked, where a scan of the files read every one. The clippy job covers Linux, the
  `cross-typecheck` job covers Windows, macOS, iOS and Android, and `wasm-check` covers wasm32.
- Bad, because the move from a counting gate to expectations edits every listed site once: the
  hand-written `catch_unwind` calls are about 400 sites in about 130 files.
- Good, because a pull request that breaks a check only main runs sees the break before it merges.
  A pull request that does not reach the check's inputs pays nothing (§9).
- Bad, because a pull request that reaches a trybuild input waits for the suites. Measured on
  main, they take about 25 minutes against the wide lane's 15, so such a pull request takes about
  9 minutes longer (§9).
- Neutral, because no size budget is enforced on `xtask`: a budget would itself be a gate, and the
  admission of §3 is where a gate's cost is weighed.

### Confirmation

- `every_self_test_reports_exactly_its_planted_findings` and
  `every_command_with_a_self_test_is_listed` in `checks_contract` (`tools/xtask/src/tasks/checks.rs`)
  run each self-test and fail on a command whose self-test is not listed (§4).
- `the_in_process_checks_include_the_link_check_under_strict` pins the list `cargo xtask checks`
  runs, which has no `--self-test` line (§4).
- `cargo clippy --workspace --all-targets -- -D warnings` fails on a hand-written `catch_unwind`
  without an expectation, and on an expectation that no longer suppresses anything (§5, §6).
- `cargo xtask workspace` checks the ADR numbers, so a gate's ADR exists; the lines of §3 are read
  in review of that ADR, the one step here no tool checks.
- `trybuild_inputs` (`tools/xtask/src/change_scope/diagnostics.rs`) checks which files reach the
  suites. A type an expectation cites, the expectation, its case, a runner and the toolchain do.
  Another source file of the same crate, documentation and tooling do not (§9).
- `send_positions_inputs` (`tools/xtask/src/change_scope/thread_positions.rs`) checks which lines
  reach `send-positions`. A changed marker, a public re-export and a listed item's name in its
  file do. Another line, a comment, a test, tooling and the same name in another file do not (§9).
- `on_demand_jobs_are_gated_on_their_output` and `an_on_demand_job_runs_exactly_when_asked` in
  `aggregator_contract` check two things: every `ON_DEMAND` job's `if:` is the condition its row
  states, and `ci-verify` fails a job that ran without being asked or skipped when asked (§9).
- `per_feature_pass_covers_the_changed_crates` in `lane_args_contract` checks the crates
  `feature-scope` covers, in a pull request's full mode too (§9).

## Pros and cons of the options

### 1. ADR-0078 §4 as the only bar (status quo)

- Good, because it needs no change.
- Bad, because it is the state the measurements above describe: the conditions admit a scan
  without asking whether a lower rung states the rule, and the code grew 3.5 times in six weeks.

### 2. The ladder, written admission, expectations at the site (chosen)

- Good, because each rung is a mechanism the toolchain already maintains, and `xtask` keeps only
  what nothing else can see.
- Good, because exceptions become compiler-checked and local.
- Bad, because moving the existing gates is work spread over several changes.

### 3. Custom lints (dylint)

- Good, because a lint written against the compiler's own representation is exact and sees types.
- Bad, because it pins a nightly toolchain and a lint crate to maintain, the reasons ADR-0078
  rejected it, and the rules in question fit stock lints or a gate.

### 4. A pattern tool (ast-grep) as the default

- Good, because a rule is a few lines of YAML instead of a parser.
- Bad, because it is a new tool every contributor installs, with its own rule language, and it has
  no per-file count or expectation, so its exceptions would return to a list.
- Neutral, because it remains a candidate under §2 rung 3 for a rule it states better than a gate;
  adopting it is its own decision.

### 5. Every rule as a test in the crate it guards

- Good, because the rule breaks where the code lies, and `cargo test` of that crate finds it.
- Bad, because the rules over the package graph, the workflows and every crate's text have no
  owning crate, and a copy in each crate would drift.

## Migration

Where each gate of `cargo xtask checks` sits on the ladder, and what follows from it. A move is its
own change; a gate that would only be removed is removed after the owner's decision.

| Gate | Rung | What follows |
|---|---|---|
| `workspace`, `reach`, `globals`, `send-impls`, `send-positions`, `try-new`, `file-length`, `testing`, `toolchain`, `docs-links`, `docs-paths`, `changelog` | 5 | stays; `try-new` has no stock lint, since `clippy::new_ret_no_self` accepts `Result<Self, _>` |
| `panic-messages` | 5 (§8) | stays |
| `inspect-debug` | 2 (§7) | replaced by the deprecated constructor, gate deleted |
| hand-written `catch_unwind` | 2 (§6) | a `clippy.toml` entry, no gate |
| `panic-boundary` | 4 (`flui-native`) | moves into `flui-native`'s tests |
| `wgsl` | 4 (`flui-engine`'s shaders) | moves into `flui-engine`'s tests |
| the freshness checks of the generators | 4 | move into the tests of the crates that hold the output; the generators stay |
| `module-dag` | 4 (each rule is about one crate's modules) | reviewed with the owner: into those crates' tests, or module visibility |
| `markers` | 5 (prose, ADR-0078 §4) | reviewed with the owner |
| `api-closure` | 3 if `cargo-check-external-types` states ADR-0089 §2, otherwise 5 | stays until that tool is measured against it |

## More information

- The `cargo xtask` pattern: <https://github.com/matklad/cargo-xtask>, which keeps `xtask` to the
  automation cargo lacks.
- `#[expect]` and `unfulfilled_lint_expectations`:
  <https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-expect-attribute>.
- clippy's `disallowed-methods` configuration:
  <https://rust-lang.github.io/rust-clippy/master/index.html#disallowed_methods>.
- A statement-level expectation of `clippy::disallowed_methods`, a crate-root
  `cfg_attr(test, allow(…))` and an unfulfilled expectation were checked on a scratch crate with
  the pinned toolchain on 2026-11-03: the production call without an expectation and the stale
  expectation are the only warnings.
- Revisit §8 if clippy gains a lint over panic messages, and the rung of `api-closure` once
  `cargo-check-external-types` is run against the facade.
