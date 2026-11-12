### Changed

- A paint pass queues a parent's child paint order only when it differs from painting every child in place, or to drop one an earlier pass kept, so the queue it keeps between frames no longer grows with every parent: a `RawButton` keeps 4966 live bytes over its `Text` instead of 5055.
