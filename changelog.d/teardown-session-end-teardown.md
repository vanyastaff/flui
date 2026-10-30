### Changed

- The ending phase of a session end now tears the UI runtimes down itself, running the
  application's destructors, when at least one second of `SESSION_END_BUDGET` is left and no UI
  runtime is checked out; it then cancels the services and shuts the pools down without waiting
  (ADR-0260 §5, §6). Otherwise they are left for the system to end with the process.
