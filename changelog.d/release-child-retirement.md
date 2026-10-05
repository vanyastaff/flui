### Fixed

- Preserve the first route parser failure when already accepted full and prefix routes have panicking destructors.
- Protect independently owned static children and retained panic payloads during child recovery and removal hooks, preserving healthy sibling progress.
