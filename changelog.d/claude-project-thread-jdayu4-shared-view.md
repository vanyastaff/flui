### Changed

- `View::create_element` takes the view as `self: Rc<Self>`, and the `ElementKind` constructors take `Rc<V>`: an element keeps its configuration in the shared allocation of the `BoxedView` that offered it, and an update takes the new allocation, so a mount, an update and a build no longer copy the view. A hand-written impl changes `fn create_element(&self)` to `fn create_element(self: Rc<Self>)`; its body is unchanged. A `RawButton` holds 4799 bytes instead of 5055, and a label change allocates 274 times over four frames instead of 282.
