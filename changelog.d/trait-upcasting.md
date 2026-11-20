### Changed

- `CustomPainter`, `FlowDelegate`, `SingleChildLayoutDelegate`, `MultiChildLayoutDelegate`,
  `SliverGridDelegate`, `BuildDuringLayoutCell`, `ViewKey`, `PlatformSystemChrome` and
  `PlatformHaptics` now have `Any` as a supertrait and no `as_any` method. An implementor deletes its
  `as_any`; a caller upcasts, `(delegate as &dyn Any).downcast_ref::<T>()`. A `&Box<dyn Trait>`,
  `&Rc<dyn Trait>` or `&Arc<dyn Trait>` is dereferenced to `&dyn Trait` first, since the pointer is
  itself an `Any`.
