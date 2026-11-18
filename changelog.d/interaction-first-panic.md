### Changed

- **`flui-interaction`**: a pointer route's first panic is a `flui_foundation::panic::FirstPanic`.
  `InteractionDispatchHandle::invoke_pointer_route` returns `Result<FirstPanic, _>` and
  `MouseTracker::dispatch_hover` returns `FirstPanic`; `resume()` raises the held panic, or
  discards it while the thread is already unwinding.

### Removed

- **`flui-interaction`**: `pointer::RoutePanic`, replaced by `FirstPanic`.
