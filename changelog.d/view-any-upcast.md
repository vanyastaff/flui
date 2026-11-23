### Changed

- `View` and `ElementBase` have `Any` as a supertrait: a `&dyn View` or `&dyn ElementBase`
  downcasts by upcasting to `&dyn Any` (`let any: &dyn Any = view;`), and an `Rc<dyn View>` or
  `Box<dyn View>` to `Rc<dyn Any>` or `Box<dyn Any>`. `flui-view` no longer depends on
  `downcast-rs` or `dyn-clone`.
- `ProxyView::child`, `InheritedView::child` and `ParentDataView::child` return `&BoxedView`, and
  `RenderView::visit_child_views` hands its visitor `&BoxedView`: a widget stores its child as a
  `BoxedView` and hands it on shared, so no child view is copied on its way to its element.
- `ViewSeq::for_each` hands each child as `&BoxedView`; its tuple, array and `Vec` impls require
  `View + Clone` elements, and a child that is not already a `BoxedView` is erased into one of its
  own clone.
- `ElementTree::mount_root`, `ElementTree::mount_root_with_pipeline_owner` and `ElementTree::insert`
  take `&BoxedView`, and the element keeps that shared allocation.
- `ChildViews::get`, `Child::as_ref`, `Children::get` and `Children::iter` return `&BoxedView`;
  `ChildViews` converts from `Vec<BoxedView>` instead of `Vec<Box<dyn View>>`.
- `RootRenderView` and `RootRenderElement` are no longer generic: the root holds its child as a
  `BoxedView`, and `RootRenderView::new` takes any view.
- `Memo<V>` holds its view shared and hands it to its proxy element without a copy.

### Removed

- `Notification::as_any` and `Notification::notification_type_id`.
- The `downcast-rs` and `dyn-clone` methods `dyn View` and `dyn ElementBase` carried: `as_any`,
  `as_any_mut`, `into_any`, `into_any_rc`, `is`, `downcast_ref`, `downcast_mut`, `downcast_rc` and
  `downcast` on the trait objects, and `dyn_clone::clone_box` and the `Clone` impl of
  `Box<dyn View>`.
