### Added

- `Subscription::for_removal`: a custom source's owned subscription, whose
  removal runs once at the first detach or the drop that follows — the value
  notification's twin of ADR-0180's status guard.
- `Listenable::listen_with_recovery`: the required relay registration under an
  owned `Subscription`, keeping ADR-0219's delivery-scope custody.
- `ChangeNotifier::add_with_recovery`: the channel-level registration for
  owners that retire through their own recovery scope.

### Changed

- The `Listenable` protocol's listener registration is owned: `listen`,
  `listen_with_recovery` and their `Subscription` replace the id-returning
  surface. The id methods remain on the trait as transition adapters — the
  production callers moved to the owned form in the same change.
- ScrollPosition's activity pair hands out the subscription
  (`add_activity_listener`); the paired `remove_activity_listener` goes.

### Removed

- The hand-rolled guards the id protocol forced on callers
  (`RenderSubscription`'s id fields, the semantics refresh-listener guard, the
  element-level `(Listenable, ListenerId)` pairs, the material components'
  `Option<ListenerId>` fields) hold the owned subscription instead.
- `ScrollPosition::remove_activity_listener` (withdraw through the
  subscription's own drop).

### Fixed

- The floating header and the tab pager's detach-on-swap paths withdraw
  through their own subscription, so a swapped controller cannot detach
  somebody else's registration (base-02's cross-channel id detach).
