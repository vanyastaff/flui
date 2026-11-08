### Fixed

- Navigator preserves route-command wakes raised while a synchronous route pop query holds the history lock. Releasing that lock now triggers the owed pump, so finishing an animated replacement from `can_pop` immediately retires its predecessor instead of waiting for an unrelated navigation operation.
