# Focus node channel — tasks

- **Feature:** [requirements](requirements.md); design: [design.md](design.md)
- **Status:** Proposed — awaiting the owner's ratification of the
  reconciliation in requirements.md (points 1–3) and the `FocusSubscription`
  question. Nothing below lands before that.
- **Base:** `main` @ the commit the first slice starts from.

| # | Slice | Depends on | Status | Evidence |
|---|-------|------------|--------|----------|
| T1 | The node list: `FocusNode`'s ledger → `ChangeNotifier`; `subscribe` → owned `Subscription`; `FocusSubscription` ends; the facade re-export follows | ratification; | Proposed | focus retention/key suites green; facade-ext fixture green |
| T2 | The manager list: `Notifier<FocusChange>`; `listen` replaces the id pair; editable-text holders become subscriptions | T1 | Proposed | interaction focus suites; editable-text rebuild rows |
| T3 | The round bridge: one close per transition, channels joining by recovery, first failure handed over at the tail; a competing-failure row | T2 | Proposed | failure-custody rows in the focus contract tables |
| T4 | Documents: ADR-0026 revision (primitives superseded by the owned channel surface), ADR-0177 round note, interaction ARCHITECTURE, `changelog.d/focus-node-channel.md` | T3 | Proposed | `rg` clean of the `add_listener(ListenerId)` teaching |
