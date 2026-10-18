### Fixed

- Preserve the running Windows xtask executable in mixed-package scoped test
  plans by testing the driver separately from feature-unified consumer packages.
- Keep Messenger subprocess readiness markers in temporary storage so tests
  also work when Cargo's target and build directories are outside the checkout.
- Isolate hot-reload tests that replace the process-global rebuild callback,
  and keep scoped runtime diagnostic capture reliable under parallel tests.
- Check Windows text-service teardown against the owned STA lifetime, allowing
  an unrelated process MTA; restrict orphan-window checks to their owner thread
  so another worktree's same-title window cannot produce a false failure.
- Accept a terminal connection reset in the HTTP test client only after the
  complete declared response arrived; keep truncated bodies and invalid HEAD
  responses failing, and bound the fixture's socket waits.
