# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

The project guide is shared by every agent runtime and lives in one place:

@AGENTS.md

## Delegating

The session keeps what needs the whole picture: the decomposition, the design decisions and the
last read of a diff before it is pushed. A subagent gets the cheapest model and effort that do
its job, both named on the spawn. Without them a definition's `inherit` runs it on the session
model, and the session's effort does not carry over to another model.

| Work | `model` | `effort` |
|------|---------|----------|
| Locate code, list callers, pull one fact from a file or a page | `haiku` | `low` |
| Run a named check and report it; an edit whose shape is settled (a rename, a move, a changelog fragment, a PR body) | `sonnet` | `low` |
| Research across sources, tests for a settled contract, a contained change with a clear end state | `sonnet` | `medium` |
| A reader of the review before a merge (AGENTS.md, "Before a merge") through the records, tests, boundaries, seams or rules lens | `sonnet` | `high` |
| Implementation that changes behaviour inside an accepted design; a reader of the review before a merge through the obligations or unwinding lens | `opus` | `high` |
| Design and ADR options, the judge of the review before a merge and any other review against "Recurring defects", diagnosis of re-entrancy, unwinding, lifetimes or races | `fable` | `high` |

Work that fails at its row is rerun one row up with what the failed run found, and the top row
at `xhigh`. Work its model refuses (a usage limit; a runtime that cannot tell a limit from a
skip or a failure treats them alike) runs on the next model down when that model does the work
(an `opus` reader on `sonnet`, the `fable` judge on `opus`), and its result says so; otherwise
the result names it as not done. A row that keeps failing one kind of work is the
row to change. A spawn spends tens
of thousands of tokens before it reads a line, so a handful of tool calls stays in the session.

`fable` is the highest alias a spawn can name. Mythos, where the account has it, is the same
model: a session running on it hands it to a fork, or to a definition's `inherit`.

- **A brief is the worker's whole context.** It states the end state and the check that proves
  it (a test by name, a gate, a command and the output expected), the files the worker owns and
  what is already ruled out. A worker told only what to do stops at the first thing that
  compiles.
- **A wrong result is a wrong brief.** Before a fan-out the brief runs on one unit and that
  result is reviewed; then the brief is corrected and rerun, since a hand patch teaches the
  remaining workers nothing.
