### Fixed

- Wayland: an application whose window the compositor does not draw (minimized, on a hidden
  workspace) no longer freezes once that window asks for its next frame: a Wayland surface now
  presents in `Mailbox` (else `Immediate`), paced by the compositor's frame callback, instead of a
  `Fifo` present that waited for the compositor to show the window again. Other surfaces keep
  `Fifo`.

### Added

- `SurfaceTarget::frame_pacing` and `FramePacing`: a window's surface target says whether its
  display or its compositor paces it (`Compositor` on Wayland), and the renderer picks the present
  mode from it when it makes the surface.
- `flui-native`'s `testing` kit has a Wayland compositor a test scripts
  (`testing::wayland_compositor::ScriptedCompositor`): it offers the globals and versions a test
  chooses, plays typed steps over a socket of its own (configure bounds, scale changes, an old
  seat, held-back frame callbacks, a removed output, a configure storm, a closed connection) and
  enforces a listed set of protocol rules; `WaylandPlatform::try_new_at` connects a test's host to
  it.

### Changed

- `flui_engine::Renderer::new` and `Renderer::with_surface_color` take the window's
  `Arc<dyn SurfaceTarget>` (`PlatformWindow::surface_target`) instead of any raw-handle owner, and
  `flui_engine::WindowTarget` is no longer public.
