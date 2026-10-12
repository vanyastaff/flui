# Web canvas input ownership implementation

Status: Approved, 2026-10-12; implemented, release checks pending.
Specification: [#1560](https://github.com/vanyastaff/flui/issues/1560).
Related repairs: #1049 and #1050. One implementation PR delivers this feature.

| Task | Dependencies | Acceptance |
|---|---|---|
| T1: Preserve producer baseline and publish approved contract | None | Live owners reproduce foreign keys, incoherent focus, dispatch bits and retired delivery; new same-canvas refusal has acceptance evidence |
| T2: Owned Web registration and routing | T1 | Transactional install/rollback, actual-node ownership, focus coherence, independent dispatch results, all-source retirement and close/reopen recovery |
| T3: Application startup and mounted recovery | T1 | Public startup RED before repair; initial addressed snapshot after observer installation; retained logical focus and healthy shortcut after native focus restoration |
| T4: Complete failure/reentry and fixture migration | T2, T3 | Browser probe retains owners; Unicode/pointer controls preserved; failure matrix includes competing failures and next operation; no stale owner delivery |
| T5: Lasting decisions and delivery proof | T4 | Architecture/changelog updated, counterfactual failures preserved, independent standards/spec reviews, scoped gates and CI reported accurately |

- [x] T1: Approved contract published; live browser baseline recorded.
- [x] T2: Owned Web registration and routing.
- [x] T3: Application startup and mounted recovery.
- [x] T4: Complete failure/reentry and fixture migration.
- [ ] T5: Lasting decisions and delivery proof.

Completion requires the full approved requirements, not only these summary rows.
The final implementation PR closes the feature and related repairs only after
producer and production-consumer acceptance is complete. Merge requires owner
authorization after review and exact-head CI.
