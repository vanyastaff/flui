### Fixed

- A key the platform reports without a physical code (`Code::Unidentified`) is
  now released as the key it was pressed as on every desktop and Web host, so a
  modifier changed while it is held no longer leaves it held in the focus
  system's key record.
