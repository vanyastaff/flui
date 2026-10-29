### Added

- **`acceptance_app` example**: a three-screen notes application (Home, Note, Settings under one
  `Router`, a Title field, a load with Retry) built on the `flui` facade alone. Its headless rows
  run as an external consumer crate (`/tests/fixtures/acceptance_app/`); the example opens it in
  a window, and `acceptance_app_faults` opens it with the panic its argument names.
