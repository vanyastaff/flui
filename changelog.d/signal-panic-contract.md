### Changed

- Signal read and update callbacks now use `FnMut` (still invoked at most once), so their captured state remains outside the unwind boundary and can be destroyed under panic containment.

### Fixed

- Signal updates that unwind after a partial mutation now invalidate every registered reader before resuming the original panic, while an explicit same-slot release remains authoritative.
- Recovered builds retain valid signal reads even when the read closure panics, allowing a later write to retry the element.
- Signal cleanup preserves the first panic across user-defined panicking destructors, and a failed frame wake is retried from shared scheduler state instead of stranding committed rebuild work.
