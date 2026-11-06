# Focus node channel — tasks

- **Feature:** [requirements](requirements.md); design: [design.md](design.md)
- **Status:** Implemented, one amendment applied: the live-round and close-round
  failure custody is the foundation receipt (`PanicRecovery` + `Retirement`),
  because `RecoveryFailure` deliberately releases no payload box (ADR-0287) —
  the close's tombstone keeps its semantics through a flag bridge
  (`CloseTombstone::preserve` from `round.has_failure`, including the
  omit-optional-delivery rule a preserving close already had),
  and `FocusClosePanic` remains only for rejection closes. The close's
  `enter_close`/`exit_close` bracketing is moved onto the tombstone itself.
- **Base:** `main` after the ratification merge.

| # | Slice | Depends on | Status | Evidence |
|---|-------|------------|--------|----------|
| T1 | The ledgers: node → `ChangeNotifier`; manager → `Notifier<FocusChange>` (public two-arg callback kept, delivered by reference with per-listener clones); `listen` replaces the id pair; `FocusSubscription` ends | this spec | Done | focus_retention's subscription rows (`subscription_withdrawal_preserves_independent_listeners`, `subscription_retirement_can_reenter_the_same_node`, `subscription_withdrawal_propagates_the_capture_failure_and_recovers`), the focus_keys suites, the editable-text rebuild rows in `owner_code`, and the facade-ext fixture (`flui-sdk` `surface`) staying green |
| T2 | The rounds: live transitions and the manager close build one `PanicRecovery` per round; channel notifies join its scope; retire-classified containment for teardown values; the tombstone flag bridge (omit-on-preserving, preserve-on-first-caught-failure, `enter_close`/`exit_close`) from `has_failure`; `FocusClosePanic` reduced to rejection closes | T1 | Done | `focus_failure_and_reentrancy_matrix` (competing queued failures, the close's mid-drain queue drop, preservation across withdrawal, bounded ping-pong), `focus_traversal_matrix`'s identity-exhaustion row, `closed_rejections_preserve_outer_failure_and_healthy_retirement`'s seven rejection paths, and `flui-runtime`'s `presentation_close_retirement_failures_preserve_focus_ime_and_siblings` (a preserving close omits its optional focus-loss delivery) |
| T3 | The documents: ADR-0314 (the owned channel surface supersedes the listener-id primitives; ADR-0026 §1 revised to point at it), interaction ARCHITECTURE (the ledgers are channels; the round statements name the receipt), `changelog.d/focus-node-channel.md` (with `flui-animation`'s README's listener paragraph, the same replacement class), spec tasks closed | T2 | Done | `rg` clean of the `add_listener(ListenerId)` teaching in ADRs, ARCHITECTURE and the facade |

Validation notes checked against the implementation: a rejection unwinding
inside a round that already owes a failure is caught and discarded by the
round's own first-failure rule (what `keep_caught`-discard did); a close
seeded preserving skips its deliveries instead of delivering under an
inheritance, which is what the round it replaced did; the tail resume order is
the close's finish then the round's — the close round leaves
`FocusClosePanic`, so nothing coexists in one container; the mid-round latch
window narrows to per-channel-notify, and the round's own failure rule absorbs
the difference (the only observable delta would be a reentrant ordinary close
running optional teardown callbacks a preserving close skips — covered by the
competing-failure row in T2).
