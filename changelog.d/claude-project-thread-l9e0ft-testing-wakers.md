### Added

- `flui-foundation` has a `testing` feature with `testing::CountingWaker` and `testing::panicking_waker`. They give a test a `Waker` that counts its wakes, runs an action, drops its captures when it retires (`CountingWaker::retiring_waker`), or panics. The workspace's own tests use them in place of hand-written `Wake` implementations.
