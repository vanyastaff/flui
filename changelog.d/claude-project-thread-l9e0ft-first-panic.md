### Added

- **`flui-foundation`**: `FirstPanic::absorb` folds one collector into another, keeping the
  earlier panic; `FirstPanic::is_held` and `FirstPanic::held` read what a collector holds
  ([ADR-0287](/docs/adr/ADR-0287-owner-state-is-committed-before-user-code-runs.md)).

### Removed

- **`flui-runtime`**: `lifecycle_state::preserve_first_lifecycle_panic`. Keep the first of
  several caught panics with `flui_foundation::panic::FirstPanic`.
