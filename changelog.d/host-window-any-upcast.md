### Changed

- **`flui-native`**: `HostWindow` has `Any` as a supertrait and no `as_any` method. A caller
  upcasts the window, `(window as &dyn Any).downcast_ref::<HeadlessHostWindow>()`; an
  `Rc<dyn HostWindow>` is dereferenced to `&dyn HostWindow` first (`&**window` from a
  `&Rc<dyn HostWindow>`), since the `Rc` is itself an `Any`.
