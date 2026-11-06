# Design: the focus node layer's listener lists are the notification channels

## What the layer is today

The focus node layer keeps two listener ledgers with the same contract at two
different shapes:

- **`FocusNode`** (`focus_scope.rs`): `listeners:
  RefCell<Vec<(ListenerId, FocusNodeChangeCallback)>>` over
  `FocusNodeChangeCallback = Rc<dyn Fn()>`, backed by an owner-local
  `IdMint<ListenerId>`. Public `subscribe` hands a separate guard type,
  `FocusSubscription { node: Weak<FocusNode>, listener_id: ListenerId }`,
  which withdraws through the weak on its own drop.
- **`FocusManager`** (`manager.rs`): `listeners:
  RefCell<Vec<(ListenerId, FocusChangeCallback)>>` over
  `FocusChangeCallback = Rc<dyn Fn(Option<Rc<FocusNode>>, Option<Rc<FocusNode>>)>`,
  same mint shape; public `add_listener` returns a `ListenerId`, withdrawn by
  `remove_listener` / `clear_listeners`.

Delivery on both is a hand-rolled snapshot loop that mirrors the notification
channel's semantics by hand: id snapshot, per-entry re-lookup so a listener
removed earlier in the same dispatch is never called ("a listener removed by
an earlier one in this same dispatch... is never called"),
`FocusClosePanic::invoke` containment, `retire` for the callbacks, and a
manager-wide failure scope.

The round, however, is one transaction the manager owns: a transition commits,
builds one `FocusClosePanic`, and fans out — the manager's own list, then the
per-node lists the focus path diff selected (`notify_focus_nodes` /
`notify_focus_path_change`), all sharing that one close. `FocusClosePanic`
carries the first failure, the `PreservingFailure` mode (the tombstone, the
`notification_failure` latch and `adopt`), and the final resume. Node and
manager deliveries adopt the manager's current mode into it
(`notification_failure_scope` / `failure.adopt`).

## The reconciliation

Split the ledger from the authority:

1. **Membership and the call loop go to the channels.** The node's listeners
   are a `ChangeNotifier` (the callback takes nothing, so no arg type is
   invented); the manager's are a `Notifier<FocusChange>` with
   `FocusChange = (Option<Rc<FocusNode>>, Option<Rc<FocusNode>>)` and the
   callback alias narrowed to read the delivered reference
   (`Rc<dyn Fn(&FocusChange)>`). Delivery-through-the-channel supplies, once:
   snapshot membership, skip-withdrawn-earlier semantics, containment per
   listener (`run_classified`-grade under the round's recovery), and deferred
   tail retirement of what the round withdraws mid-dispatch.
2. **The round authority stays owner-side.** The manager's transition round
   keeps building one `FocusClosePanic` per commit and drives the fan-out in
   today's order; each channel's notify joins a `PanicRecovery`-bridged
   recovery (the channels' `inherit_failure`/first-failure latch matches
   `adopt`/`preserving`), and the round's first failure hands to the close
   through `keep_caught` (or `keep_earlier`, where a close already owes one).
   The close finishes and resumes exactly as today; the
   `notification_failure` latch keeps latching from `failure.preserving()`.
3. **Closed-owner refusal stays an owner entry check.** A closed node or
   manager refuses admission before the channel sees it (the check + `retire`
   + `finish` move verbatim into the new `listen`), so the tombstone rejection
   modes keep today's meaning and the channel never delivers to a closed
   owner. Removal takes the entry through the channel's `take_callback` in a
   `clear`-grade withdrawal; `clear_listeners` becomes the channel's
   `dispose_and_take_callbacks` on the manager's close path plus a plain
   withdrawal for tests.
4. **The public surface narrows.**
   - `FocusNode::subscribe` returns the channel's `Subscription`; the node's
     Weak-holding `FocusSubscription` guard ends (its withdrawal reason —
     reaching the node through a Weak — is the subscription's own channel
     Weak now). The facade (`flui-sdk`, `flui::foundation` surface where
     re-exported) drops `FocusSubscription` in the same change.
   - `FocusManager::listen` replaces `add_listener`/`remove_listener`;
     callers (flui-text's `editable_text` focus and IME listeners, the focus
     tests) hold `Subscription`s or detach them exactly as the id-holding
     callers did. `listener_count` reads the channel's `len`.

## Why the channels match the round's semantics

- The "removed by an earlier listener in the same dispatch is never called"
  rule is the channel delivery's own skip-withdrawn rule
  (`notify_selection`'s per-entry membership check); today both loops re-lock
  the ledger per id to reproduce it.
- `FocusClosePanic::invoke`'s containment equals the channel delivery under a
  recovery scope: one first failure, later ones discarded, destructors reached
  only through the round's retirement.
- The `notification_failure` mode latch is owner state, not ledger state: the
  guards stay where they are and keep their meaning.

## What deliberately does not change

- The transition commit sequence and its fan-out order (manager list, then
  the node sets the path diff selects) — the round keeps its shape; only who
  stores and calls the callbacks changes.
- The focus-close-time listener delivery (`notify_close_listeners`) stays on
  the close's own machinery: a close's teardown delivery is the owner's
  retirement, not a live subscription round.
- ADR-0177's ordered-delivery contract: the manager keeps ordering its
  deliveries; the channel is a per-set ledger inside that order.
- The interaction layer's `Send`/`Sync` allowlists and the
  `thread_boundary_ui` compile-fail contract.

## Slices

Each slice lands green on its own gates (`clippy -D warnings`, nextest for
flui-interaction, flui-widgets, flui-text dependents, and the material/fixture
suites).

- **T1 — the node list.** `FocusNode`'s ledger becomes a `ChangeNotifier`;
  `subscribe` hands the owned `Subscription`; the internal `add_listener` /
  `remove_listener` pair (the manager's own node listeners and the focus tests)
  move to held subscriptions or channel withdrawals; `FocusSubscription` ends
  with the facade re-export. Evidence: `focus_retention`, the node rows of
  `owning_status_subscription_contract`-grade focus contracts, and the
  facade-ext fixture staying green.
- **T2 — the manager list.** The manager's ledger becomes
  `Notifier<FocusChange>`; `listen` replaces the id pair; the
  editable-text holders become `Subscription`s. Evidence: the focus
  retention/keyboard suites and the manager's own failure-mode rows.
- **T3 — the round bridge review.** Every focus round reads:
  one close, channels joining it by recovery, the first failure resumed once
  at the close's end, the mode latch latched from the round's own preserving
  state. Evidence: the failure-custody rows added in T2 plus a dedicated
  competing-failure row.
- **T4 — the documents.** ADR-0026's listener-primitive paragraph is revised
  (owned channel surface at the node layer; this design supersedes
  `remove_listener(ListenerId)`), ADR-0177's round statement notes the
  per-set channels inside the manager's order, the interaction ARCHITECTURE
  names the ledgers as channels, and the changelog fragment
  (`focus-node-channel.md`) lands with the facade note.
