# The focus node layer's listener lists are the notification channels

A live spec for moving the focus layer's two listener ledgers onto the
notification channels, revising ADR-0026's listener primitives and keeping
the focus round's failure authority where it is. The design below requires the
owner's ratification before any code moves; the ADR revision happens in the
slice that lands it, not before.

## Ratification points

1. The node layer's membership and call loop become the notification
   channel's: a `FocusNode`'s listeners live in a `ChangeNotifier`; the
   `FocusManager`'s live in a `Notifier<FocusChange>` whose arg carries
   `(previous: Option<Rc<FocusNode>>, new: Option<Rc<FocusNode>>)` and whose
   listeners read it through the delivered reference.
2. `FocusClosePanic` stays the focus round's failure authority: the manager's
   transition round keeps building one close per commit, the channels join its
   recovery (their first failure hands over at the round's tail through
   `keep_caught`/`keep_earlier`), and the close modes (`Ordinary`,
   `PreservingFailure`), the tombstone rejections and the
   `notification_failure` latch keep today's meaning untouched.
3. The public surface changes: `FocusNode::subscribe` returns the channel's
   `Subscription` (`FocusSubscription` as a separate guard type ends; the
   facade stops re-exporting it), and `FocusManager::add_listener` /
   `remove_listener` / `clear_listeners` become `listen(callback) ->
   Subscription` plus the subscription-guard tail the callers already hold —
   no `ListenerId` is public at the focus node layer any more.

Questions the ratifier should weigh: (a) whether the manager's round-failure
bridging should use `NotificationFailureGuard`'s latch exactly as today (the
design keeps it), and (b) whether `FocusSubscription` should end or remain as
a deprecated alias for one release.
