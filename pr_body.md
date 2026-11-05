## Summary

The owned `Subscription` becomes the `Listenable` registration protocol (T05a-2, base-02): every production caller migrated off the bare-id surface, the hand-rolled per-crate guards hold the foundation's subscription instead, and the seven sites the audit named are collapsed except where a recovery obligation stays load-bearing.

- `Listenable` keeps two required methods: `listen` (a plain callback) and `listen_with_recovery` (ADR-0219's relay custody — the delivery's `RecoveryScope`). Both return the `#[must_use]` `!Send` `Subscription`, which detaches through its own channel on drop; `detach` for the early exit.
- `Subscription::for_removal` gives a custom source (its storage is no foundation listener set) the same owned handle by carrying the removal — the value notification's twin of ADR-0180's status guard, which the `ReentrantParent`/`TerminalParent` fixtures and the harness's `RemovalNotifier` exercise.
- An unmounted or closed source gates the registration before the channel can answer with its disposed state (`Persisted`, `RouterHandle`), handing back `Subscription::inert()`; `ConstantAnimation` does the same on principle.
- The pairs moved: scroll activity (header host, pager link, drag inner), page/scrollable command and fling listeners, tab pager relays, modal-route relays, hero-flight wakes, settle listeners, the refresh overlay, the leader watches (watches carry `(watch, subscription)`), the text field and the buttons' state listeners, the semantics refresh and scroll subscriptions, devtools' overlay, and the widget/contract tests over the whole workspace.
- `ScrollPosition::add_activity_listener` now returns the subscription; the paired `remove_activity_listener` is deleted. The widget-pair shape (`Option<(Rc<dyn Listenable>, ListenerId)>` across `page_view` and the animated element) became the owned `Option<Subscription>` with the swap-path commentary about cross-channel id detach removed with the defect.

## Deliberately kept (deviations to review)

- **`RenderSubscription` stays a recovery owner**: its close runs under its caller's `PanicRecovery` and retires the source under containment — reducing it to a bare `Subscription` field would move the retirement into the subscription's drop without the caller's first-failure custody. Its id fields are gone; registration and detachment go through the owned handle, and the admission gate stays for the revoked-relay case. (base-02 named it; the audit's collapse loses containment.)
- **The trait still carries `add_listener`/`add_observer`/`remove_listener`/`remove_all_listeners`** as transition adapters for the animation contract tests and the foundation's channel-level rows — the production callers and every widget/element path moved off them in this change. The rest of the id-method retirement (the contract-test sweep) follows in the next part of T05a; until then the trait's registration story is owned-only.
- **`FocusSubscription` and the runtime's `LifecycleSubscription`/`HostNoticeSubscription`** stay: they wrap channels that are not foundation `Listenable`s (the focus node's own hub, the runtime's `Observation`), so the collapse does not apply; ADR-0179/ADR-0219 own those surfaces.

## Contracts pinned by tests

- The subscription detaches only its own channel (`a_subscription_detaches_only_its_own_channel`), idempotently (`..._once_and_a_second_detach_is_a_noop`), quietly on a disposed or gone channel, and inside-its-round is skipped and retired at the tail without holding a borrow.
- The rebuild-swap paths: `editable_key_edit_whose_listener_removes_on_changed/_replaces_.../_replaces_the_controller` and the floating-header/pager links — a swap withdraws through the subscription's own channel, so a controller's id space cannot reach someone else's registration.
- The self-withdrawing listener retires inside the delivered round's containment; its capture moved in drops there, and `raised` at the dispatch and at the next tick sees the owner's failure first.

## Validation

- `cargo heli nextest` over foundation…packages: 1433 passed (the trybuild row runs solo green — the 7-job parallel cold-cache artifact-lock flake this branch's logs already carry);
- clippy `--all-targets -- -D warnings` clean workspace-wide; fmt clean;
- the trybuild compile-fail suites for painting and platform pass solo.
