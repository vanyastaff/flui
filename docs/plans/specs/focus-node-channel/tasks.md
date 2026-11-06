# Focus node channel — tasks

- **Feature:** [requirements](requirements.md); design: [design.md](design.md)
- **Status:** Ratified with one amendment: the live-round and close-round
  failure custody is the foundation receipt (`PanicRecovery` + `Retirement`),
  because `RecoveryFailure` deliberately releases no payload box (ADR-0287) —
  the close's tombstone keeps its semantics through a flag bridge
  (`CloseTombstone::preserve` from `round.has_failure`), and
  `FocusClosePanic` remains only for rejection closes. The manager's
  `close`/`close_with_mode` therefore moves to foundation custody in the same
  change; `close.rs`'s tombstone/`CloseMode` types stay owner-side.
- **Base:** `main` after the ratification merge.

| # | Slice | Depends on | Status | Evidence |
|---|-------|------------|--------|----------|
| T1 | The ledgers: node → `ChangeNotifier`; manager → `Notifier<FocusChange>` (public two-arg callback kept, delivered by reference with per-listener clones); `listen` replaces the id pair; `FocusSubscription` ends | this spec | Proposed | focus retention/key suites; editable-text rebuild rows; facade-ext fixture |
| T2 | The rounds: live transitions and the manager close build one `PanicRecovery` per round; channel notifies join its scope; retire-classified containment for teardown values; the tombstone flag bridge from `has_failure`; `FocusClosePanic` reduced to rejection closes | T1 | Proposed | failure-custody and competing-failure rows; close rows; `close.rs` untouched except callers |
| T3 | The documents: ADR-0026 revision (primitives superseded by the owned channel surface), ADR-0177 round note, interaction ARCHITECTURE, `changelog.d/focus-node-channel.md`; spec tasks closed | T2 | Proposed | `rg` clean of the `add_listener(ListenerId)` teaching |

Validation notes carried from the investigation: a rejection unwinding inside
a round that already owes a failure is caught and discarded by the round's own
first-failure rule (what `keep_caught`-discard did); the tail resume order is
the close's finish then the round's — a parked-store payload kept through
`keep_earlier` wins exactly as today, because the close round leaves
`FocusClosePanic`, so nothing coexists in one container; the mid-round latch
window narrows to per-channel-notify, and the round's own failure rule absorbs
the difference (the only observable delta would be a reentrant ordinary close
running optional teardown callbacks a preserving close skips — covered by the
competing-failure row added in T2).
