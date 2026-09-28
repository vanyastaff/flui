### Changed

- Signal read, update, and cross-thread command callbacks now use `FnMut` (still invoked at most once), so their captured state remains outside the unwind boundary; successful callbacks destroy it only after framework finalization is durable, while panic recovery retains opaque capture bundles that Rust cannot safely dismantle generically.

### Fixed

- Signal updates that unwind after a partial mutation now invalidate every registered reader before resuming the original panic, while an explicit same-slot release remains authoritative.
- Recovered builds retain valid signal reads even when the read closure panics, allowing a later write to retry the element.
- Signal cleanup preserves the first panic across user-defined panicking destructors, and a failed frame wake is retried from shared scheduler state instead of stranding committed rebuild work.
- A full realm command inbox retries outstanding wake debt before returning backpressure, so an accepted command cannot remain stranded behind a permanently full queue.
- Signal replacement, reader invalidation, equality comparison, and value destruction now run as separate phases; framework loan, invalidation, redraw, and FIFO-rearm state becomes durable before opaque user aggregates are destroyed, while already-failing paths retain values that cannot be dismantled generically.
- Rebuild and realm wake debt now uses replaceable identity tokens instead of finite integer generations, preserving overlapping-delivery ordering without a 32-bit saturation failure.
