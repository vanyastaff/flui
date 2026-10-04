### Fixed

- Animation controllers sample custom simulations and curves outside their state lock, reject stale reentrant samples, and retire displaced sources and status callbacks after releasing the lock.