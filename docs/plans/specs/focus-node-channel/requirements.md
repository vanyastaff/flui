# The focus node layer's listener lists are the notification channels

A live spec for moving the focus layer's two listener ledgers onto the
notification channels, revising ADR-0026's listener primitives and keeping
the focus round's failure authority where it is. The design below requires the
owner's ratification before any code moves; the ADR revision happens in the
slice that lands it, not before.

## Ratification points

1. The node layer's membership and call loop become the notification
   channels: a `FocusNode`'s listeners live in a `ChangeNotifier`; the
   `FocusManager`'s live in a `Notifier<FocusChange>` whose arg carries
   `(previous: Option<Rc<FocusNode>>, new: Option<Rc<FocusNode>>)` and whose
   listeners read it through the delivered reference — the manager's public
   two-argument callback shape stays (channel callbacks deliver it by
   reference and the manager clones per listener, exactly as the id-form
   delivered by value).
2. A live transition round's failure authority is the foundation's
   `PanicRecovery` (the ADR-0287 receipt), not the close: the channels join
   the round's recovery scope, the round's first failure resumes once at the
   round's tail — the same points where `FocusClosePanic::finish` resumes
   today — and a rejection inside a round that already owes a failure is
   discarded by the round's own first-failure rule, as the close discarded
   it. `FocusClosePanic` keeps the surfaces that are owners of their own
   custody: closed-owner admission refusals and the close-time teardown
   delivery (`close`/`close_with_mode`), whose shape is unchanged. The
   `notification_failure` latch latches from the round's `has_failure` at the
   same guard boundaries.
3. The public surface changes: `FocusNode::subscribe` returns the channel's
   `Subscription` (`FocusSubscription` as a separate guard type ends; the
   facade stops re-exporting it), and `FocusManager::add_listener` /
   `remove_listener` / `clear_listeners` become `listen(callback) ->
   Subscription` — no `ListenerId` is public at the focus node layer any
   more.

Questions the ratifier should weigh: (a) whether the manager's round-failure
latching should use `NotificationFailureGuard`'s boundaries exactly as today
(the design keeps them, scoping the latch per channel-notify), and (b) whether
`FocusSubscription` should end or remain as a deprecated alias for one release.
