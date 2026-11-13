### Changed

- `flui_native::OwnerPlatform::on_wake` takes a `Box<dyn FnMut()>`: the owner-turn callback may hold owner-local state (`Rc`, `Cell`), waits between turns in the host loop's own slot and is dropped on the owner thread (ADR-0327 §5).
