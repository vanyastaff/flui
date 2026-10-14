### Fixed

- `AnimatedSize` restores its size owner before reporting a failed deferred registry move, so a panicking tracing subscriber can no longer leave it without an owner.
