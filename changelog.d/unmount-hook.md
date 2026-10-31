### Added

- `LifecycleContext::on_unmount(hook) -> UnmountHook`: a state hands its element's unmount to a value
  it opens. The hook runs once, right after `dispose`, in registration order, at a removal, a
  subtree eviction and a tree torn down at exit or session end, never at a `GlobalKey` move. Each
  hook is contained, and a panic is recorded as the element's `dispose` failure. Dropping the token
  unregisters the hook; `UnmountHook::forget` keeps it registered.

### Changed

- A `Persisted` document's listeners are dropped when the state that opened it unmounts, so a
  listener may hold a clone of the document without keeping it alive.
