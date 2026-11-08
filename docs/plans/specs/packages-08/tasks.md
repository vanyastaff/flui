# Tasks: typed component themes

- **Status:** Complete
- **Date:** 2026-11-08
- **Requirements:** [requirements.md](requirements.md)
- **Design:** [design.md](design.md)

| Task | State | Validation evidence |
|---|---|---|
| T1. Add typed inherited-entry dependencies to `flui-view` | done | Ten entry tests cover exact invalidation, reset, lifecycle, misses, reparent and panic recovery. Removing inactive-retake ancestry recomputation made `entry_read_reroutes_after_global_key_reparent` read `Some(1)` instead of `Some(2)`; restored behavior passed. Full suite: 165 passed, 51 ignored; strict Clippy and diff-check passed |
| T2. Replace Material component fields with typed storage and migrate readers | done | Deliberately disabled `Theme::changed_entries` failed the replacement assertion (`[1,1,1,1]` vs `[2,1,1,2]`); restored behavior passed. Final run r821: 65 passed, 5 ignored, doctests passed; r822: strict Clippy passed; r823: panic-messages passed without allowlist growth |
| T3. Align Cupertino `of`/`maybe_of` missing-ancestor policy | done | Run r819: 15 passed, 3 ignored, doctests passed; r816: strict Clippy passed, including no-ancestor and nearest-resolved-theme coverage |
| T4. Write ADR-0318 and synchronize architecture/changelog records | done | ADR, three architecture ledgers, spec and changelog synchronized; replacement searches passed; r825: docs-links, docs-paths, markers and changelog checks passed |
| T5. Run formatting, Clippy, affected test suites and replacement audit | done | r817 `flui-view`: 75 passed, 50 ignored; r821 Material: 65 passed, 5 ignored; r819 Cupertino: 15 passed, 3 ignored; strict Clippy r816/r822 and earlier `flui-view` gate passed; r825 full `xtask checks` and final `git diff --check` passed |
