### Changed

- A presentation's animation registry is driven through `flui_animation::__runtime::VsyncDriver`, which owns the root registry and alone ticks it (`tick_all`, `tick_all_with_recovery`), binds it (`set_frame_requester`, `set_build_phase`) and closes it (`prepare_close`, returning `VsyncRetirement`) (ADR-0382). The `Vsync` handle a tree receives keeps registration, nesting, muting and its read-only queries. Migration: a test that ticked a registry it created declares `let driver = VsyncDriver::new();`, registers on `driver.vsync()` and ticks `driver`.

### Removed

- `Vsync::tick_all`, `Vsync::tick_all_with_recovery`, `Vsync::set_frame_requester`, `Vsync::set_build_phase` and `Vsync::prepare_close`; `VsyncRetirement` left the crate root for `flui_animation::__runtime`. `flui_sdk::animation` and `flui::animation` no longer reach the frame driver's seam.
