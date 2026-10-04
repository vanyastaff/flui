### Fixed
- Keep the original layout failure authoritative when a tracing subscriber panics, and retain opaque panic payloads at both leaf and nonleaf layout boundaries so their destructors cannot replace the failure or abort reporting.
