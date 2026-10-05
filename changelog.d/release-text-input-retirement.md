### Fixed

- Text input client retirement permits callback and store destructor reentry, preserves the first cleanup failure, and retains deferred editing grants for the next commit anchor until the presentation closes.
