## Summary

Describe the change and why it belongs in FLUI.

## Verification

- [ ] `cargo xtask check-changed` (CI runs the rest)
- [ ] New or changed behavior has tests that would fail without this change
- [ ] Public API changes are documented
- [ ] A cross-crate contract change is recorded (ADR), or not applicable

## Architecture

- [ ] Layering still follows `docs/FOUNDATIONS.md`
- [ ] No new lock on per-node render state, no capability acquired outside lifecycle hooks (AGENTS.md)
- [ ] New dependencies are declared through workspace dependencies when shared

## Notes

Call out deferred work, or follow-up issues.

