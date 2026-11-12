### Changed

- `ObjectKey::new` takes `&Rc<T>` for any `T: 'static` and holds the `Rc`; it no longer asks
  `Send + Sync` (ADR-0323 §4).
- `StreamBuilder` takes the bounds of `FutureBuilder`: `K: Clone + PartialEq + Debug + 'static`,
  `T, E: 'static`, and `BoxedResultStream` has no `Send` bound, so a stream may hold `Rc` state.
- `WidgetsBindingObserver::did_pop_route` and `did_push_route_information` return futures
  without `Send`; they are polled on the owner thread.
