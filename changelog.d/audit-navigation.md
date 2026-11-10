### Added

- **Typed page results** (`flui-widgets`): `RouterHandle::push_for_result::<T>` pushes a page and
  returns its `RouteResult<T>`, and `RouterHandle::pop_with` pops with a value (ADR-0093 §3). The
  result resolves to `Ok(Some(value))`, to `Ok(None)` for a page that leaves without one (a pop, a
  replace, a `go` that drops it, its router unmounting), and to
  `RouteResultError::TypeMismatch` for a value of another type.

### Changed

- **`RouteResult<T>` is owner-thread state** (`flui-widgets`): it is an `Rc`/`RefCell` channel and
  no longer `Send`, and route results need no `Send`: the bound is gone from `Route::Output`,
  `PageRoute`, `PopupRoute`, `SimpleRoute`, the `NavigatorHandle` doors that take a result,
  `show_dialog` and `cupertino_page_route`, and neither `Send` nor `Clone` is needed of `T`.
- **A route result is a `Result`** (`flui-widgets`, `flui-material`): `RouteResult<T>` resolves to
  `Result<Option<T>, RouteResultError>` and `try_take` answers
  `Option<Result<Option<T>, RouteResultError>>`; `OpenContainer::on_closed` receives the same
  `Result`.

### Fixed

- **A mismatched pop no longer reads as "no value"** (`flui-widgets`): a route popped with a value
  of another type completes with `RouteResultError::TypeMismatch` instead of `None`.
- **Unmounting a navigator no longer strands awaiting tasks** (`flui-widgets`): every result still
  pending resolves to `Ok(None)` and wakes its task, after the history lock is released; a
  panicking waker or observer leaves the others delivered and is raised once teardown has run.
