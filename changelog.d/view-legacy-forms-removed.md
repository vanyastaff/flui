### Removed

- **`flui-view`**: the empty `ElementExt` marker trait.
- **`flui-view`**: `test_only_set_global_key_registry` and `test_only_clear_global_key_registry`, and the `IntoView` and `IntoElement` impls for `Box<dyn View>`; a view is erased with `.boxed()` or `BoxedView::new`.

### Changed

- **`flui-view`**: `ErrorViewBuilder` returns a `BoxedView` instead of a `Box<dyn View>`.
- **`flui-view`**: the field of `BoxedElement` is private; `BoxedElement::into_inner` reads it.
- **`flui-view`**: `flui_view::testing::activate_global_key_registry` scopes a `GlobalKey` registry for a test and deactivates it when its guard drops.
