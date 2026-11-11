### Changed

- `ChildViews::one` and `ChildViews::push` take any view by value, and hold a `BoxedView` shared, without a box of its own; the single-child, proxy, render and root elements hand on such a child shared. `ChildViews::as_slice` is replaced by `len`, `get` and `iter`.
