### Changed

- `ChildViews` hands on a child the element holds as a `BoxedView` shared, without a box of its own: `ChildViews::shared` and `push_shared` take one, and the single-child, proxy, render and root elements use them. `ChildViews::as_slice` is replaced by `len`, `get`, `iter` and indexing.
