### Fixed

- Preserve the first caught listener failure during reentrant ancestor callback retirement in nested animation relays, while restoring normal destruction on the next healthy notification.
- Keep synchronous recovery scopes on the stack so deep animation wrapper composition preserves allocation-free steady-state frames.
