### Fixed

- Keep background workers progressing after replaced-result retirement or diagnostic panics, and retain opaque lifecycle failures safely while preserving completion evidence.

### Changed

- A worker future whose poll panicked, and lifecycle values retired after an earlier failure, are retained instead of dropped (ADR-0127): their destructors never run, so resources they own (a socket, a channel sender) stay open until exit.
