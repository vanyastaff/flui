## Summary

Describe the change and why it belongs in FLUI.

## Verification

- [ ] `cargo fmt --all`, clippy and nextest on the changed crates (AGENTS.md, "Checking a change"; CI runs the rest)
- [ ] New or changed behavior has tests that would fail without this change
- [ ] Public API changes are documented
- [ ] A cross-crate contract change is recorded (ADR), or not applicable
- [ ] A consumer-visible change adds a `changelog.d/<branch-slug>.md` fragment, or not applicable

## Expectations

One line per path `cargo xtask expectations` lists: the path (or its directory, with the trailing
`/`) and why the old expectation was wrong or the new one is right. "None" when it lists nothing.

## Review

Who read the diff in a fresh context before the merge (AGENTS.md, "Before a merge"), each finding
and what became of it: fixed, or refuted by a line or a test. "Documentation only" when the `docs`
lane takes the pull request.

## Architecture

- [ ] No new `edge-exceptions`, `reach-exceptions` or allowlist entry without the ADR that removes it
- [ ] No new lock on per-node render state, no capability acquired outside lifecycle hooks (AGENTS.md)
- [ ] New dependencies are declared through workspace dependencies when shared

## Notes

Call out deferred work, or follow-up issues.

