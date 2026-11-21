### Changed
- `RenderObject<P>` and `ParentData` take `Any` as a supertrait instead of `downcast_rs::Downcast`; `dyn RenderObject<P>` and `dyn ParentData` keep inherent `is`, `downcast_ref` and `downcast_mut`, and anything else downcasts by upcasting the trait object to `&dyn Any`.

### Removed
- The `downcast-rs` methods `as_any`, `as_any_mut`, `into_any`, `into_any_rc` and `downcast` on `dyn RenderObject<P>` and `dyn ParentData`; upcast to `dyn Any` instead.
