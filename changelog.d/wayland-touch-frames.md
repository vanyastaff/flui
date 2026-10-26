### Changed

- On a Wayland session the winit backend delivers the touches of one loop turn as one `PlatformInput::PointerFrame`, so a pinch is measured once per compositor dispatch. winit does not report `wl_touch.frame`; a turn holds whole frames.
