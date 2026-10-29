### Security

- Update the locked tokio dependency to 1.54.1, resolving RUSTSEC-2026-0384 (a race between
  `tokio::time::Interval` and the multi-threaded time driver) reported by the dependency
  advisory gate.
