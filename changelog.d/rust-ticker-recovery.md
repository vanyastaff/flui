### Changed
- Ticker completion callbacks now accept `FnMut` and run exactly once; callbacks that consume captured values can use `Option::take` explicitly.
- Ticker futures support wakers that poll or drop futures inline, with one removable registration per pending future and independent registrations for clones.

### Fixed
- Ticker delivery preserves the first failure across callback, waker, destructor and reporting failures while continuing healthy callbacks and waiters. Opaque captures and secondary payloads are intentionally retained during recovery.
