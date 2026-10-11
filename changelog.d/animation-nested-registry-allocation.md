### Fixed

- Nested animation registries reuse empty frame-walk storage and avoid allocating
  child snapshots during active-run queries. Detached child owners still retire
  individually outside registry borrows, including after a muted walk or callback
  failure.
