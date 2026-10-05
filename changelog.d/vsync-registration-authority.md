### Changed

- Vsync registration tokens now carry backend ownership, implement Clone instead of Copy, and are borrowed by removal methods. Use `unregister(&token)` and `detach_child(&token)`.

### Fixed

- Foreign and stale Vsync tokens cannot remove unrelated work; exhausted registries permanently refuse new identities while accepted animations continue. `try_register` reports typed capacity refusal.
- Renderer semantics-listener storage and dispatch snapshots preserve the first propagated failure when independent callback captures fail during retirement, while healthy retirement and subsequent notification remain available.
- Retire removed Vsync controllers and child registries after releasing their registry mutex, allowing last-owner destructors to reenter safely.
