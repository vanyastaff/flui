# ADR-0379: An animation status subscription is the foundation `Subscription`

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0180](ADR-0180-source-bound-animation-status-subscriptions.md)
- **Related:** [ADR-0109](ADR-0109-notification-channel-surface.md) (the value channel's owned
  `Subscription`), [ADR-0178](ADR-0178-notification-first-failure-custody.md) (first-failure
  custody), [ADR-0179](ADR-0179-controller-registration-ownership.md),
  [ADR-0219](ADR-0219-required-observer-registration.md) (relays borrow the delivery's scope)

## Context and problem statement

ADR-0180 bound status removal to its source with a type of the animation crate,
`StatusSubscription`: a weak source, a `Copy` token and a removal function pointer. The foundation
then gained its own owned `Subscription` for value listeners (ADR-0109), with a closure-based twin
for custom sources, `Subscription::for_removal`. The two handles named one idea differently and
disagreed on one verb:

- `StatusSubscription::detach` left the callback registered for the source's lifetime, while
  `Subscription::detach` removes the listener at once. A caller who learned one read the other
  backwards.
- An animation is `Listenable`, so a reader saw `listen` and `listen_with_recovery` beside
  `subscribe_status` and `observe_status`: four verbs for two pairs of the same shape.
- `for_removal` held a closure, so a custom source could capture itself strongly in its own
  removal and keep itself alive, which ADR-0180's guard was built to rule out.

## Decision drivers

- One owned handle for every listener registration in the framework, with one meaning per verb.
- A handle never keeps its source alive and holds no user captures.
- Removal inside a cleanup round keeps that round's first failure (ADR-0178).
- The names follow `Listenable`'s, which every animation already implements.

## Considered options

1. **Keep `StatusSubscription`**, renaming its `detach` to say what it does.
2. **Return the foundation `Subscription`**, which gains ADR-0180's source-bound form in place of
   `for_removal`, a scoped detach and a verb for leaving the callback with its source.
3. **Register status callbacks on a foundation `Notifier` and return its channel subscription**,
   dropping the source-bound form.

## Decision

Chosen option: 2, because it leaves one handle and one vocabulary, and the source-bound form is
the stricter of the two custom forms. Option 1 keeps two types for one idea, and every widget that
holds both kinds still writes two field types. Option 3 loses the owners' own removal: a proxy or
switch withdraws into its own deferred retirement during its delivery, which a channel detach
does not know about.

```rust
// flui_foundation
pub struct Subscription { /* channel, source-bound removal, or nothing */ }

impl Subscription {
    pub fn for_source<S: ?Sized + 'static, Token: Copy + 'static, Outgoing: 'static>(
        source: &Rc<S>,
        token: Token,
        remove: fn(&S, Token, &mut RecoveryScope<'_>) -> Outgoing,
    ) -> Self;
    pub fn inert() -> Self;
    pub fn detach(self);
    pub fn detach_with_recovery(self, recovery: &mut RecoveryScope<'_>);
    pub fn forget(self);
}

// flui_animation
pub trait Animation<T>: Listenable {
    fn listen_status_with_recovery(&self, observer: StatusObserver) -> Subscription;
    fn listen_status(&self, callback: StatusCallback) -> Subscription { .. }
    // value, status, is_animating
}
```

1. **One handle.** `Animation::listen_status` and `listen_status_with_recovery` return
   `flui_foundation::Subscription`. Dropping it or calling `detach` withdraws the registration at
   once; `forget` gives up the right to remove it, and the source's disposal or retirement
   removes the callback. There is no public status id and no source-selected removal method.
2. **Source-bound removal.** `Subscription::for_source` holds the source weakly, a `Copy` token and
   a removal function pointer, so it cannot keep the source alive and holds no user captures. A
   source that reuses storage refuses a stale token. The removal commits withdrawal with the
   source's state borrows released and returns the outgoing callback; the subscription releases
   its temporary source reference before destroying that callback inside the detaching scope. A
   capture destructor that releases the last owner therefore closes the source's channels before
   anything re-enters them. Deferred delivery custody stays in the source's own queue and the
   removal returns an empty value. `for_source` replaces `for_removal`.
3. **Scoped detach.** A detach that belongs to a cleanup round already under way calls
   `detach_with_recovery` with that round's scope, so the round's first failure stays the one it
   resumes. A plain `detach` or drop runs its own recovery and resumes its first failure after the
   withdrawal has committed.
4. **Owners.** Reverse, curved and tween animations bind removal to their shared parent-links
   owner; proxy and switch animations bind it to their own owners, and their observers stay
   subscribed across parent replacement and active-parent hops. The last wrapper clone closes its
   channels even when a subscription survives. A disposed controller or switch refuses admission
   with `Subscription::inert()` and retires the rejected captures outside its state borrows; an
   exhausted status channel refuses through the foundation `IdMint`.
5. **Relays.** Framework status relays register with `listen_status_with_recovery` and borrow the
   enclosing delivery's `RecoveryScope`; only the enclosing `PanicRecovery` finishes the first
   failure.

### Confirmation

`owning_status_subscription_contract` covers independent sources and channels, self and later
removal during delivery, `detach` removing at once, `forget` keeping the callback until the owner
retires, a surviving subscription after source teardown, a custom source built with `for_source`,
capture reentry, failure custody and the progress after it. Its wrapper rows cover clone lifetime,
proxy parent replacement and a switch hop, and its enclosing-cleanup row checks that
`detach_with_recovery` keeps an established failure. `status_delivery_contract` and
`status_delivery_failure_custody` cover delivery and retirement through every owner.

## Pros and cons of the options

### 1. Keep `StatusSubscription`, renamed

- Good, because no caller outside the verb changes.
- Bad, because two handles name one idea, and a widget holding both kinds keeps two field types.
- Bad, because the foundation's closure form still lets a custom source pin itself.

### 2. The foundation `Subscription` with a source-bound form

- Good, because there is one handle and one meaning per verb across value and status listeners.
- Good, because the stricter custom form, no captures and a weak source, is the only one.
- Bad, because every status caller and every custom `Animation` changes its signature.

### 3. Channel subscriptions only

- Good, because the foundation needs no custom form at all.
- Bad, because a proxy or switch removing during its own delivery would drop the callback in place
  instead of handing it to its deferred retirement, so a capture destructor could re-enter a
  delivery in progress.
