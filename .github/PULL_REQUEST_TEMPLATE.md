## Summary

Describe the change and why it belongs in FLUI.

## Verification

- [ ] `cargo fmt --all`, clippy and nextest on the changed crates (AGENTS.md, "Checking a change"; CI runs the rest)
- [ ] New or changed behavior has tests that would fail without this change
- [ ] Public API changes are documented
- [ ] A cross-crate contract change is recorded (ADR), or not applicable
- [ ] A consumer-visible change adds a `changelog.d/<branch-slug>.md` fragment, or not applicable

## Review

The head the readers and the judge read (AGENTS.md, "Before a merge"), each finding the judge
confirmed or left unaccounted for (every reader's finding when no judge started) and what became
of it: fixed, with its class, or refuted by a line or a test; the same for each later read of a
fix. A lens that could not be started, with its reason and what the author's own read through
it showed. "Documentation only" when the `docs` lane takes the pull request;
`Independent read unavailable: <reason>` and the scenarios the author tried, when no reader
could be started.

## Architecture

- [ ] No new `edge-exceptions` or `reach-exceptions` entry without the ADR that removes it
- [ ] No new lock on per-node render state, no capability acquired outside lifecycle hooks (AGENTS.md)
- [ ] New dependencies are declared through workspace dependencies when shared

## Notes

Call out deferred work, or follow-up issues.

