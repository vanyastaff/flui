### Fixed
- Request/reply slots run executor cloning and wake retirement outside locks, publish completion before waking, and preserve the first failure during abandonment or owner disconnection. Exceptional failure paths retain opaque captures instead of running competing destructors.
