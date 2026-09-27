### Fixed

- Signal updates that unwind after a partial mutation now invalidate every registered reader before resuming the original panic, while an explicit same-slot release remains authoritative.
- Recovered builds retain valid signal reads even when the read closure panics, allowing a later write to retry the element.
