# ADR-0331: A pull request names every expectation it edits

- **Status:** Accepted (2026-11-13)
- **Date:** 2026-11-13
- **Related:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (the ladder this gate is admitted under); [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  §4 (a gate reads structured data and its allowlist only shrinks);
  [ADR-0324](ADR-0324-the-facade-api-is-a-committed-snapshot.md) (the API snapshot, one of the
  expectations); [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) (the golden protocol schemas,
  another)

## Context and problem statement

A check compares the code with an expectation someone committed: the API snapshot `api/flui.txt`,
a trybuild `*.stderr`, an insta `*.snap`, a golden protocol schema, a perf baseline, an allowlist
of a gate, a test, an exemption in `deny.toml` or `typos.toml`. A change that edits the
expectation moves what the check accepts, so the check passes whether the edit records a decision
or hides a defect. Each gate sees only its own half: `tools/xtask/src/ratchet.rs` says that an
allowlist entry "added or raised in the same change as the code it excuses … shows up only in the
diff of the allowlist file", and nothing reads that diff. An author merges their own pull request
once CI is green, and most authors are agents working toward a green run, for which editing the
expectation is always the shortest way there.

Such edits are frequent and mostly legitimate, which is why they pass unread. The 31 commits
`a0ba4b9f8..ee16d28bb` on main hold 21 of them in 18 files: 3412 lines removed from the API
snapshot, 13 trybuild expectations changed or deleted, two perf baseline counts raised, four tests
fewer in one file, four new `send-positions` entries. No pull request of that range says which of
them it meant.

What makes an edit to an expectation a stated decision, in the pull request that makes it?

## Decision drivers

- A green run proves the code against the expectations, so the expectations are the one part of
  a change a green run says nothing about.
- The author knows the reason and writes it in a minute; a reader who has to reconstruct it from
  the diff does not.
- A pull request does not wait: the check compiles `xtask` alone and reads git.
- Rules are types first and an `xtask` gate last (ADR-0303 §2); a gate reads structured data, not
  patterns over source text (ADR-0078 §4).
- The edits are legitimate far more often than not: the rule asks for a reason, not for
  permission.

## Considered options

1. A check that lists the expectations a pull request edits and fails unless the body names each
   path with its reason.
2. The same list as a report in the job summary, failing nothing.
3. `CODEOWNERS` on the expectation files, so each such pull request waits for the owner.
4. An expectation changes only in a pull request of its own, apart from the code.
5. No rule: whoever reviews reads the diff.

## Decision

Chosen option: 1, because it is the only one that puts the reason on record without making a
pull request wait for a person, and the only one a reader can check afterwards: the reason stands
next to the path in the merged pull request.

### §1. What an expectation edit is

`cargo xtask expectations` (`tools/xtask/src/change_scope/expectations.rs`) compares the merge
base with the pull request and lists:

| Expectation | Counts as an edit |
|-------------|-------------------|
| `api/flui.txt` | lines the snapshot no longer has (an added line is new API, which its own diff shows) |
| a trybuild `*.stderr`, an insta `*.snap` | the file changed or deleted (an added one is a new case) |
| `crates/flui-protocol/tests/schema/*.json` | the file added, changed or deleted: a published schema is frozen and a new one is a protocol version |
| `perf/size.toml`, `*/perf/baseline/*.toml` | a ceiling that rose (a size, a package count, a cost counter of `cargo xtask perf`), a work count that changed either way (it has to match, so a fall is work that stopped happening), a count no longer written, or the file deleted |
| `tools/xtask/allowlists/*.toml` | an entry with no counterpart before, a count that rose, a list of names that gained one |
| a Rust file | fewer tests than before: test attributes (`#[test]`, a path ending in `test`, `#[wasm_bindgen_test]`, also under `cfg_attr`) plus table-test cases (`case as fn()`); or more `#[ignore]` attributes |
| `deny.toml`, `typos.toml` | a value the file did not have: a new key, a new array item, a value added to an entry, a changed setting (an entry is named by its own fields, wherever it stands; a `reason` is prose and is not compared) |

A file is compared whole, before and after, so moving a test inside a file or turning attributes
into a table of cases is no edit, and moving tests to another file is one the body explains by
naming where they went. The list is of this pull request's edits only: nothing is counted across
pull requests, so there is no allowlist of its own to keep.

### §2. The body names each path

A pull request that edits an expectation has an `## Expectations` section in its body. Each
edited path has a line there holding the path and why the old expectation was wrong or the new
one is right; a line holding a directory with its trailing `/` covers the files directly in it,
so one line explains a regenerated trybuild suite. A line with fewer than twelve letters and
digits besides the path is not a reason. `cargo xtask expectations --body <file>` fails on each
path the section leaves unnamed.

The check judges that a reason is present, not that it is good. Its use is that the reason is
written before the merge by the one who knows it (`AGENTS.md`, "Before each push", item 14),
and that a reader of the diff starts from a list instead of a search.

### §3. Where it runs

`.github/workflows/expectations.yml` runs the command on a pull request's `opened`, `edited`,
`reopened` and `synchronize` events, with the body passed as a file. It is a workflow of its own
because editing the body has to run it again and must not run `ci.yml` again. Locally,
`cargo xtask expectations` prints the list against `origin/main`, and `--worktree` includes
uncommitted work.

### §4. Admission

Under ADR-0303 §3, one line per rung:

1. **A type** cannot state a rule about the difference between two commits.
2. **No rustc or clippy lint** sees a deleted test, a data file or a pull request's body; a lint
   judges one tree.
3. **A tool CI already runs:** cargo-deny, typos, taplo and lychee each read one tree.
   `cargo-semver-checks` compares two, for one crate's API and nothing else here.
4. **A test of the owning crate** runs inside one tree and cannot see the merge base or the body;
   each expectation's own test is the check this rule guards from being moved.
5. The gate reads what no crate's build sees: git's list of changed files, both versions of each,
   and the pull request's body.

Under ADR-0078 §4: no type or stock lint states it (above); it reads git's NUL-separated name
and status list, the token trees of Rust files (`proc_macro2`, so a `#[test]` in a comment or a
string is not a test and one inside `proptest! { … }` is), parsed TOML tables, and the lines of
the generated snapshot, each of which is one item's full path; the body is prose and is read as
text, for the section and the path only. Its cases are the table test `expectations_contract`
in `cargo test -p xtask`; it has no `--self-test` flag because it has no tree to scan a planted
violation in. It keeps no allowlist.

### Consequences

- Good, because every edit to an expectation that reaches main has its reason beside it, written
  by its author, and a change that only weakens a check can no longer be merged without saying
  so.
- Good, because the list is the first thing a reviewer of the diff reads: it is where a green
  run proves nothing.
- Bad, because a pull request that narrows the public API, regenerates trybuild output or blesses
  a baseline writes a line per file or per directory, in a repository where such pull requests
  are common before 1.0.
- Bad, because a reason can be empty words; the check cannot tell. A reviewer can, and has the
  line to hold the author to.
- Neutral, because weakening an assertion inside a test that stays is not an expectation edit
  here: no tool tells a weaker assertion from a corrected one. Tests are counted per file, so a
  test replaced by another in the same file is not listed either.
- Neutral, because a pull request's check is not the squash commit's message: the reasons live
  in the pull request.

### Confirmation

- `cargo xtask expectations --body` in `.github/workflows/expectations.yml`, on every pull
  request.
- `expectations_contract` (`tools/xtask/src/change_scope/expectations.rs`) in
  `cargo test -p xtask`: each kind of edit is found, its harmless twin (an added line, a fall, a
  move, a reworded reason, a conversion to a table) is not, and a body names a path only inside
  the section, alone and with a reason.

## Pros and cons of the options

### 1. A check that fails unless the body names each edit

- Good, because the reason is recorded where the change is, by the author, before the merge.
- Good, because it costs a pull request no waiting: it reads git and compiles `xtask`.
- Bad, because it asks for prose a tool cannot grade.

### 2. A report that fails nothing

- Good, because it adds no way for a pull request to fail.
- Bad, because nothing makes an author read it: a report nobody has to answer is the diff
  nobody reads, one click further away.

### 3. `CODEOWNERS` on the expectation files

- Good, because a person decides.
- Bad, because most pull requests that touch the API or a compile-fail case would wait for the
  owner, which is the wait the self-merge rule exists to remove.
- Bad, because it covers files, not lines: a deleted `#[test]` in an ordinary source file has no
  path to own.

### 4. Expectations change only in their own pull request

- Good, because the edit is impossible to miss.
- Bad, because a change to the API and its snapshot, or to a diagnostic and its `*.stderr`,
  cannot be split: the first half is red on its own.

### 5. No rule

- Good, because it costs nothing.
- Bad, because it is the state the context describes.

## More information

- GitHub, "Migrating the GitHub Copilot runtime to Rust, using Copilot" (2026-09-16),
  <https://github.blog/ai-and-ml/migrating-the-github-copilot-runtime-to-rust-using-copilot/>:
  "you must have tests that can be used to validate the correctness of the port, and those tests
  can't themselves be re-written during the porting, or else you lose your oracle."
- Revisit §2 if reasons turn out to be filler in practice: the next step is a required owner
  review for the kinds that stay unexplained, not a stricter word count.
