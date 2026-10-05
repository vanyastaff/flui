### Fixed

- Animation controllers, proxies, curved animations, switches and tickers withdraw owned callbacks, curves and subscriptions before terminal retirement. Independent remaining captures are retained after the first destructor failure or during an existing unwind; healthy destruction and shared aliases retain their normal behavior.

### Changed

- `flui_animation::curve::Split` owns its curves in private guarded fields and no longer implements `Copy`. Replace field literals or mutation with `Split::with_curves`, read configuration through `split()`, `begin_curve()` and `end_curve()`, and use `clone()` where a copy was required. Deserialization now checks the same finite `[0, 1]` split range as construction.
