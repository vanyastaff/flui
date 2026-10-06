### Fixed

- Commit complete focus-owner teardown before final notifications, prevent reentrant callbacks from repopulating closed nodes, and retain outgoing captures after a failure or during unwinding so their destructors cannot replace the original panic.
- Skip removed global key handlers during the current dispatch even when callers still retain them, while continuing through remaining registered handler identities.
