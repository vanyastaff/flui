### Added

- Down callbacks can retain a weak, owner-affine pointer capture token to route later contact packets exclusively to their target. Releasing the token preserves accepted movement and delivers one capture-loss cancellation on the owning presentation; stale tokens cannot cancel replacement contacts.
- Released contact tails are matched to their device, so another device's hover remains deliverable when a native pointer ID is reused.
- Capture release wakes the owning presentation after committing cancellation debt; wake failures preserve that debt and accepted motion. Listener unmount preserves its admitted contact's terminal cleanup. The token controls logical routing and does not perform explicit OS capture release.
