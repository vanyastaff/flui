### Changed

- `ElementBase::build_into_views` returns `ChildViews` instead of `Vec<Box<dyn View>>`: `ChildViews::one` holds a single child without a list, `ChildViews::push` appends one, `ChildViews::none()` holds none, and a `Vec` converts with `From`.
- An element offered a new view of its own type clones the view itself instead of boxing a copy and unboxing it, and a proxy or inherited view whose child is a `BoxedView` hands on that same shared view instead of copying it into a fresh box.
