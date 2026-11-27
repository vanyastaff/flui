### Changed

- `ParentData` no longer names `dyn_clone::DynClone` as a supertrait; `flui-rendering` no longer
  depends on `dyn-clone`. A parent data type is still any `Debug + Clone + 'static` type with
  `impl ParentData for T {}`, and `Box<dyn ParentData>` is still `Clone`.

### Removed

- `Clone` for `Box<dyn ParentData + Send>`, `Box<dyn ParentData + Sync>` and
  `Box<dyn ParentData + Send + Sync>`, which `dyn-clone`'s macro added and nothing used.
