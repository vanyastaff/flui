### Fixed

- Animation controllers, proxies, curved animations, switches and tickers withdraw owned callbacks, curves and subscriptions before dropping them. After the first destructor failure, or while the thread is already panicking, the remaining values are retained and never dropped (ADR-0127); healthy destruction is unchanged.

### Changed

- `flui_animation::curve::Split` owns its curves in private guarded fields and no longer implements `Copy`. Replace field literals or mutation with `Split::with_curves`, read configuration through `split()`, `begin_curve()` and `end_curve()`, and use `clone()` where a copy was required. Deserialization now checks the same finite `[0, 1]` split range as construction.
