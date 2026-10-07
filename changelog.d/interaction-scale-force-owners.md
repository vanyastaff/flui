### Changed

- Configure scale and force-press recognizers with immutable builders before sharing their `Rc` owners. Both accept full pointer dispatches, support reusable cancellation, and silently withdraw arena membership when dropped.
