### Added

- `RouterHandle::push_for_result<T>` pushes a typed `PageRoute<T>` and returns an owner-local, `!Send` `RouteResult<T>` backed by `Rc<RefCell<_>>`. Route outputs need only `'static`, not `Send` or `Clone`; the result implements `Future` and supports `try_take()`. `pop_with<T>` supplies a typed result, and `maybe_pop() -> Result<bool, RouterError>` preserves `PopScope` and the last-page guard.
- `RouteResultError::TypeMismatch { expected: &'static str }` reports a mismatched pop value without preventing the pop. Resultless pop, `replace` and `go` complete pending typed results with `Ok(None)`.

### Changed

- Terminal navigator disposal completes every pending route result exactly once with `Ok(None)`, running owner-local completions outside the history lock without route lifecycle callbacks, observer notifications or Router stack edits. Deactivation and reactivation leave results pending, and later stale-handle route lifecycle remains separate from result completion.
