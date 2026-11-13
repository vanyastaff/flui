### Changed

- `flui_native::HostPlatform::run_shared` takes `Rc<Self>`, and `Platform::on_keyboard_layout_change` takes a `Box<dyn FnMut()>`: a platform and its loop live on the owner thread, so neither is shared through `Arc` and the callback may hold owner-local state (ADR-0327 §5).
