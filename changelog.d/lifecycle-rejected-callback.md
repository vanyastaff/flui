### Fixed

- Retain rejected lifecycle callback captures when an earlier drain failure or independent unwind owns the error, and release source borrows before ordinary rejected-callback destruction.
