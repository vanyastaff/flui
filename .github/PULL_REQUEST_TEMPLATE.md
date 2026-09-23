## Summary

Describe the change and why it belongs in FLUI.

## Verification

- [ ] `cargo xtask check-changed` (CI runs the rest)
- [ ] Flutter reference checked for render/layout/paint/lifecycle/reconciliation changes, or not applicable
- [ ] New or changed behavior has tests that would fail without this change
- [ ] Public API changes are documented
- [ ] A Flutter divergence is recorded (ADR or `## Mapping decisions`), or not applicable

## Architecture

- [ ] Layering still follows `docs/FOUNDATIONS.md`
- [ ] No new lock on per-node render state, no capability acquired outside lifecycle hooks (AGENTS.md)
- [ ] New dependencies are declared through workspace dependencies when shared

## Notes

Call out intentional divergences, deferred work, or follow-up issues.

