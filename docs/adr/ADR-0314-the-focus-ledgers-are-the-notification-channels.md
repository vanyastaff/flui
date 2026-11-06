# ADR-0314: The focus ledgers are the notification channels

- **Status:** Accepted (2026-11-06)
- **Date:** 2026-11-06
- **Supersedes:** [ADR-0026](ADR-0026-focus-traversal-seam.md) §1 only (the node layer's
  listener primitives: `add_listener`/`remove_listener(ListenerId)` and the
  `FocusSubscription` guard do not exist; listeners hold the owned
  [`Subscription`](../../crates/flui-foundation/src/notifier.rs))
- **Related:** [ADR-0178](ADR-0178-notification-first-failure-custody.md) (the channel
  delivery contract these ledgers join),
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (the receipt a round
  holds), [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md)
  (destructor containment at retirement),
  [ADR-0177](ADR-0177-controller-ordered-delivery.md) (the ordered-round shape the focus
  manager keeps; the channels are per-set ledgers inside that order)

## Context and problem statement

The focus node layer stored two listener ledgers by hand: `FocusNode`'s
`RefCell<Vec<(ListenerId, FocusNodeChangeCallback)>>` behind an owner-local
`IdMint`, and `FocusManager`'s same shape for change listeners. Each duplicated
the notification channel's semantics by hand — id snapshots, per-entry
re-lookup so a listener removed earlier in the same dispatch is never called,
`FocusClosePanic::invoke` containment, retirement of the invoked callbacks, and
a manager-wide failure-scope latch (`notification_failure_scope`). The same
duplicated shape had already been retired in the animation and tab controllers,
which now sit on the foundation channels and their owned `Subscription`s.

The focus layer additionally mixes two authorities in one hand-rolled type:
the ledger (who is subscribed, in which snapshot delivery runs) and the round
(the transaction a transition commits under, whose first failure is resumed at
its tail). `FocusClosePanic` carried both, plus the preserving mode and the
close's tombstone authority, in different shapes per call site.

Which crates: `flui-interaction` (the focus layer), every widget that holds a
focus subscription, and the facade's re-exports.

What should own membership, delivery and withdrawal?

## Decision drivers

- Rules are types: a hand-maintained ledger of `Rc` callbacks with a parallel
  id list repeats every containment rule at every call site and keeps drift
  possible between the three copies (node, manager, close).
- ADR-0178 made the foundation channels the notification primitive; the
  `Listenable` trait carries the owned pair (`listen` → `Subscription`).
- Pre-1.0: fix a bad shape rather than work around it.
- The frame path is synchronous — unchanged; the channels are owner-local
  `Rc` values (`Cell`/`RefCell`), no lock on per-node state.

## Considered options

1. Keep the hand-rolled ledgers and port the round to the foundation receipt
   alone, keeping ids.
2. Move membership and delivery to the channels and leave the round authority
   in `FocusClosePanic`.
3. Move membership and delivery to the channels and move round authority to
   the foundation receipt (`PanicRecovery`), keeping the owner-side tombstone
   for rejection policies.

## Decision

Option 3.

**The ledgers.** `FocusNode`'s listeners are a `ChangeNotifier`; its public
`subscribe` is defined on the node and returns the channel's owned
`Subscription`, whose drop detaches through the channel's `Weak` — the same
reasoning `FocusSubscription`'s weak-node guard had, without a second guard
type or any listener id. A closed node refuses the admission before the
channel sees it and retires the offered listener under a rejection close's
custody, exactly as the id form refused. `FocusManager`'s change listeners are
`Notifier<FocusChange>`; the public two-argument callback is untouched — the
manager's `listen` accepts the user closure, the channel delivers
`&FocusChange` by reference, and the subscribing closure clones the pair per
listener, so delivery-by-reference costs the same as the id form's
delivery-by-value. `add_listener`/`remove_listener`/`clear_listeners` end;
callers hold subscriptions, and the manager's close path disposes the channel
(`dispose_and_take_callbacks`) to retire everyone it still holds. The facade
stops re-exporting `FocusSubscription` and exports the callback alias.

**The rounds.** A live transition round is one foundation
`PanicRecovery` (`focus_round`), seeded from the manager's notification-failure
latch. Every channel notify joins the round's `RecoveryScope`
(`notify_listeners_with_recovery` on the node, `notify_with_recovery` on the
manager); the foundation latch keeps each channel's nested-delivery failure
state. A rejection unwinding inside a round that already owes a failure is
caught by the round and discarded at `finish` — what the close's
`keep_caught`-discard did. `FocusClosePanic` leaves the live rounds and keeps
two roles it already had: each **closed-owner admission refusal** (a
self-contained `for_rejection` close at the refusal site, before any channel
sees the request) and — replaced by the receipt — the **close round** below.
The `notification_failure` latch is owner state and latches from
`round.has_failure()` at the same guard boundaries.

**The close.** `close_with_mode` commits terminal state, then runs one
foundation round seeded from its close mode:

- A close that owes a failure — a preserving mode, itself inside an unwind
  (`std::thread::panicking()`), or a tombstone a previous close left
  preserved — omits the optional notifications entirely (the node ledgers'
  final deliveries and the manager's `(previous, None)` delivery) and still
  retires every outgoing capture through the round's containment. This is the
  old `FocusClosePanic::run`'s omit-on-preserving rule.
- The first caught failure of an ordinary close turns the rest preserving at
  once: later nodes and the manager's own delivery hear nothing more, and the
  tombstone is preserved immediately, so reentrant rejections made by
  destructive captures still read preserving while the close runs.
- The tombstone keeps its meaning through a **flag bridge**:
  `CloseTombstone::preserve` from `round.has_failure()` at the round's tail
  (and, for a mid-round failure, at the moment the failure is caught), with
  `enter_close`/`exit_close` bracketing the close as before. `RecoveryFailure`
  deliberately releases no payload box (ADR-0287), so a hand-over of the
  payload into the owner-side tombstone is exactly what the receipt design
  forbids; the bridge carries the policy, never the payload.
- A close that landed mid-round leaves the manager's channel disposed; later
  callers in that round hear nothing, where the emptied id ledger of the
  removed form silenced them the same way.

**Ordering.** The transition commit sequence and its fan-out order are
unchanged: the manager's channel first, then the node sets the focus path diff
selects; the first failure resumes at the round's tail after the accepted
reentrant-queue drain.

## Alternatives rejected

- **Option 1 (ids kept):** keeps the hand-written per-entry re-lookup and
  three copies of containment; the channels already carry exactly these rules
  and the layer above them was already migrated (animation status, tab
  controller), leaving the focus layer as the only hand-rolled copy.
- **Option 2 (`FocusClosePanic` keeps the round):** one type still carries
  ledger, custody and mode authority, so the owner keeps two delivery
  machineries and the close keeps its own; a receipt-owning round is the
  foundation primitive both neighbours already use, and `RecoveryFailure`'s
  no-payload-box rule makes a payload hand-over into the close impossible
  without breaking ADR-0287's reason.
- **A separate `FocusSubscription` kept as an alias:** a second guard type
  over the same withdrawal; its weak-node reach was the channel's own weak
  channel handle, so the alias teaches nothing the `Subscription` does not.

## Verification and limits

The `focus_traversal_matrix` row `focus_node_identity_exhaustion_preserves_notifications`
holds both endpoints' subscriptions (`held_listeners`) and pins independent
delivery at a refused mint; `focus_failure_and_reentrancy_matrix` pins the
failure-custody rows (node and manager origins, competing queued failures,
listener replacement under a failed round, queue drops when the manager closes
mid-drain, preservation of a caught failure across withdrawal);
`closed_rejections_preserve_outer_failure_and_healthy_retirement` covers all
seven rejection paths; the close rows cover healthy close, the first close
failure, retirement-panic competition, and the flag bridge's preserving
rejections. Subscription-holding rows (`subscription_withdrawal_propagates_the_capture_failure_and_recovers`)
pin that a healthy withdrawal propagates its capture destructor's failure —
and that a destructor panicking during an active unwind aborts, as in std
(ADR-0281): a withdrawal made inside an unwind is not defended.

These regressions fail with the production changes reverted. The limits:
A panic inside a listener's destructor while the thread is already unwinding
aborts — the channels carry no defence beyond what std accepts (ADR-0281);
the mid-round latch window is per-channel-notify, narrower than the removed
whole-round guard, and the round's own failure rule absorbs the difference;
the facade's `Send`/`Sync` allowlists and the `thread_boundary_ui`
compile-fail contract are unchanged by this design.
