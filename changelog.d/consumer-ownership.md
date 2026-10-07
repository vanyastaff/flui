### Fixed

- A transition route dropped mid train-hop without `dispose` no longer leaks its secondary animation proxy.
- Disposing a transition route no longer deadlocks when a status-listener capture reads the route's controller from its `Drop`.
