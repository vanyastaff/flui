# ADR-0323: The facade's public API is a committed snapshot every pull request keeps current

- **Status:** Accepted (2026-11-12)
- **Date:** 2026-11-12
- **Related:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (every item the facade
  reaches carries the Stable promise; this record lists them); [ADR-0136](ADR-0136-owner-local-ui-surfaces.md)
  §3 (`send-positions`, which walks the same surface); [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (the ladder this gate is admitted under); [ADR-0153](ADR-0153-release-train-and-capability-crates.md)
  (cargo-semver-checks on the platform contract)

## Context and problem statement

`flui` is a Stable crate (ADR-0089), and its modules are whole internal crates
(`pub use flui_widgets as widgets`, `pub use flui_foundation as foundation`, `pub use flui_view::*`
in `src/lib.rs`). Whatever those crates make `pub` is part of the promise: about 5.4 thousand
public declarations, `unsafe` constructors of identity types among them. ADR-0089 §1 says "the
closure gate of §6 is what lists them", but no gate does: `api-closure` checks which upstream
crates the facade's own items name and does not follow a re-export into another workspace crate,
and `send-positions` walks the whole surface but reports only its `Send` and `Sync` bounds. A pull
request that adds, removes or changes a public item shows nothing of it unless a reviewer reads
every `pub` in the diff, and the narrowing of the crate roots that has to happen before the first
publication has no record to narrow against. What records the facade's surface so that every
change to it is visible in the pull request that makes it?

## Decision drivers

- The surface is a promise: a change to it is a decision someone makes on purpose, in review.
- A pull request shows the change; a main-only check shows it after the merge, to the next author.
- Rules are types first and an `xtask` gate last (ADR-0303 §2); a gate earns its place by ruling out
  each rung above it (ADR-0303 §3).
- One loader for rustdoc's JSON: `api-closure` and `send-positions` already build and read it.
- No new workspace crate and no tool every contributor installs for one check.

## Considered options

1. A committed text snapshot of the facade's surface, written and checked by `cargo xtask
   public-api` over the loader `send-positions` uses, checked on every pull request.
2. `cargo-public-api` with its own snapshot.
3. `cargo-semver-checks` on `flui` against the base commit, as the `doc` job does for
   `flui-platform`.
4. No record: review reads the `pub` items of each diff.

## Decision

Chosen option: 1, because it is the only one that follows the facade's re-exports into the
workspace crates they name, and it adds no tool and no build to what `send-positions` already
does.

### §1. The snapshot

`api/flui.txt` lists, one line each, what a user can name through `flui` with every feature on, as
the Linux host builds it: each module; each type with its public fields or variants and whether it
has private fields; each function with its signature; constants, statics, type aliases and macros;
under a type, its public inherent items and every trait implementation an author wrote, with the
associated types it sets; under a trait, its items and its implementations for types nothing else
lists (a blanket `impl<T: Send> Trait for T`). `#[non_exhaustive]` and `#[deprecated]` are written
on the line, because they change what a user may write.

An item gets one path, the first the walk finds in order of path length; each other path to it is
a `reexport` line naming that one, so the prelude adds one line per name and not a copy of each
item. A path in a signature is written as the facade path of the item it names; an item the facade
does not reach keeps the path rustdoc records (`flui_engine::…`), so a signature that leaks an
engine type reads as one. A `#[doc(hidden)]` item, a synthetic implementation (`Send`, `Unpin`) and
a blanket implementation's copy on each type are not written.

### §2. The gate

`cargo xtask public-api` (`tools/xtask/src/public_api.rs`) renders the surface and fails, listing
each line that differs, unless the text equals `api/flui.txt`; `--write` restates the file. It
reads rustdoc's JSON through `api_closure::rustdoc_json` with the builds `send-positions` asks for
(every feature, private items), so the two share their cost; its `--self-test` renders the
document `send-positions` plants its positions in and a stale snapshot, and runs in
`cargo test -p xtask` (ADR-0303 §4).

Admission under ADR-0303 §3, one line per rung:

1. **A type** cannot state which items a crate exports; the compiler accepts any `pub`.
2. **No rustc or clippy lint** records a crate's public items; `missing_docs` and
   `unreachable_pub` judge items one at a time, against no record.
3. **A tool CI already runs:** `cargo-semver-checks` reports breaking changes against a baseline
   but not additions, which are most of what the narrowing has to stop, and keeps no record a pull
   request diffs.
4. **A test of the crate** cannot run rustdoc's JSON output (nightly) over every crate the facade
   reaches from inside `cargo test -p flui`.
5. The gate reads what no crate's build sees: the rustdoc JSON of every crate the facade's
   re-exports reach.

### §3. Where it runs

The `public-api` job of `ci.yml` runs on every pull request that compiles anything (the `wide`
lane) and on main: `cargo xtask public-api`, then `cargo xtask send-positions` over the same
builds. A pull request that changes the surface carries the regenerated `api/flui.txt`, and its
diff is the review of the change. `api-closure`, which builds the core host for every target,
stays in the main-only `api-contracts` job.

### Consequences

- Good, because every pull request shows what it adds to and removes from the facade, and the
  narrowing of the crate roots is a diff that only shrinks the file.
- Good, because `send-positions` moves onto pull requests with no build of its own, so its list
  no longer turns main red after a merge.
- Bad, because a pull request that compiles anything waits for a nightly rustdoc build of the
  26 crates the facade reaches (about 11 minutes cold on a 4-core runner, uncached: the builds
  take 5 GB), in parallel with the longer `test` job,
  and a change to the surface needs a nightly toolchain locally to restate the
  file.
- Bad, because a rustdoc JSON format change can rewrite many lines at once; the renderer reads
  the JSON untyped and writes `?` for a shape it does not know, so such a change is visible, not
  silent.
- Neutral, because only the host's build is recorded: an item another target builds (Android's
  `run_app_android`, the Windows host's extensions) is not in the file.

### Confirmation

- `cargo xtask public-api` in CI's `public-api` job, and
  `public_api_fixture_renders_as_expected_and_reports_the_planted_lines` with the
  `every_self_test_reports_exactly_its_planted_findings` table in `cargo test -p xtask`.
