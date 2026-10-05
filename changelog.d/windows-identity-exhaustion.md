### Fixed

- Permanently refuse exhausted Windows backend identities before acquiring native windows, preventing identity reuse and an HWND leak after caught capacity failures.
