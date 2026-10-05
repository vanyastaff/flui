### Fixed

- Permanently refuse exhausted scheduler task identities before admission, preserving cancellation authority and retaining rejected callbacks or futures during the capacity panic.
