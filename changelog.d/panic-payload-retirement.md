### Fixed

- Contained layout panics release ordinary static-string and owned-string payloads after reporting, preventing diagnostic-path memory leaks. Arbitrary panic payloads remain retained to avoid competing destructor panics; the original poisoned-layout error remains authoritative.
