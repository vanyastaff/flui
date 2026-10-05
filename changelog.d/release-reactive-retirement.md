### Fixed

- Signal release completes graph bookkeeping before destroying user values, allowing destructor reentry and retaining remaining owned values after the first teardown failure.
