# ADR-0356: A surface its compositor paces presents without waiting on the compositor

- **Status:** Implemented (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0353](ADR-0353-production-is-paced-by-the-platform-and-a-frame-that-showed-nothing-waits-for-the-fallback.md)
  decision 0 only, its first sentence (`Fifo` for every surface, `Mailbox` unreachable): a Wayland
  surface presents in `Mailbox`, else `Immediate`. The rest of the decision (the loop waits,
  `target_fps` is advisory, the frame-latency setting) stands.
- **Supersedes:** [ADR-0347](ADR-0347-a-frame-request-carries-the-hosts-frame-times-and-reports-that-name-their-frame.md)
  decision 5 only (`Fifo` stays the only present mode): this is the record that decision named, for
  the pacer that does not rely on the present call blocking.
- **Supersedes:** [ADR-0044](ADR-0044-driver-loop-hybrid.md) §4 only, the pacer of its Wayland
  row ("the blocking Fifo present"): the compositor's frame callback alone paces a Wayland surface.
- **Supersedes:** [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) decision 1's
  public bound only: `Renderer::new` takes the window's `Arc<dyn SurfaceTarget>`, and
  `WindowTarget` is the lease's crate-private bound.
- **Supersedes:** [ADR-0254](ADR-0254-window-contract.md) §5's clause that `Renderer::new` keeps
  ADR-0063's `WindowTarget` bound: it takes the surface target, which says who paces the surface.
- **Related:** [ADR-0254](ADR-0254-window-contract.md) §5 (`SurfaceTarget::pre_present_notify`
  arms the frame callback), [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §5 (the window's slot configures its surface), [ADR-0243](ADR-0243-one-testing-kit-per-layer.md)
  (the scripted compositor that confirms it is `flui-native`'s testing kit)
- **Sources:** Mesa 25.2.8's Wayland WSI,
  [`wsi_common_wayland.c:3002`](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.8/src/vulkan/wsi/wsi_common_wayland.c#L3002)
  (the `wl_display_dispatch_queue` loop a `Fifo` present waits in) and
  [`:3097`](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-25.2.8/src/vulkan/wsi/wsi_common_wayland.c#L3097)
  (the frame callback it waits for, below `wp_fifo_v1`); Raven at `7ab270df`,
  [`platform.h:359-373`](https://github.com/oscar-soirey/Raven/blob/7ab270df3b618de3d11af6033912374b6cd621b7/src/platform/platform.h#L359-L373)
  (a compositor-paced present without driver vsync)

## Context and problem statement

The native Wayland host paces a window by its compositor: `pre_present_notify` arms the surface's
frame callback with every present, and the window asks for its next frame only once the compositor
answers it, or once `CALLBACK_GRACE` (one second) has run out, because a present that commits
nothing never brings its callback (`flui-native` `window::linux::wayland::frame`). A compositor
answers no callback for a surface it does not draw: a minimized window, one on a hidden workspace,
one covered on a compositor older than `xdg_toplevel` version 6's `suspended`.

The engine configured every surface `Fifo`. A `Fifo` swapchain on Wayland waits for the same
answer: Mesa's, where the compositor has no `wp_fifo_v1` (every compositor before GNOME 48 and
KWin 6.3), loops in `vkQueuePresentKHR` until the previous present's frame callback comes, with no
bound. So the frame the host delivers once the grace runs out blocks in its present until the
compositor shows the window again, and the raster executor runs that present on the owner thread:
input, timers, every other window and a quit wait with it. A run against the scripted compositor
of `flui-native`'s testing kit showed it: an animating application whose window the compositor
never draws does not carry out the quit it asks for. (A resize does not show it: the swapchain it
recreates starts with no present outstanding, so the wait comes only with a second present on one
swapchain.)

How does a surface present so that no present waits on a compositor that does not draw it?

## Decision drivers

- The frame path is synchronous and the owner thread runs the present (ADR-0305): a present that
  waits on a peer holds the whole application.
- The loop blocks until woken (ADR-0249), so a present mode that never blocks needs a pacer that
  does not rely on the present call: ADR-0347 decision 5 asks for exactly that before `Mailbox`
  enters.
- On Wayland that pacer exists: the host withholds the next frame until the callback the present
  carried is answered, so one frame per compositor refresh is the most a window draws.
- On X11 and Win32 nothing else paces a frame: there the `Fifo` block is the pacer (ADR-0353).

## Considered options

1. The pacer chooses the present mode: a surface its compositor paces presents in `Mailbox`, else
   `Immediate`, else `Fifo`; a surface the display paces presents in `Fifo`.
2. Keep `Fifo` everywhere and drop the callback's grace, so a window whose callback never comes is
   never asked for another frame.
3. Keep `Fifo` everywhere and present on a thread of its own, so the owner never waits on it.
4. Require `wp_fifo_v1`, whose barrier a compositor releases for a surface it does not draw.

## Decision

Chosen option: 1, because the compositor's frame callback already paces a Wayland surface, so the
present has nothing left to wait for, and the other options either freeze a window a skipped
present left waiting or move the wait instead of removing it.

Who paces a surface is a fact of the host that arms (or does not arm) its frame callback, so the
surface target states it, and the engine reads it once, when it makes the surface:

```rust
// flui-platform
#[non_exhaustive]
pub enum FramePacing {
    Display,    // a present that waits for the display's refresh is the pacer (the default)
    Compositor, // the host withholds the next frame until the present's frame callback is answered
}

pub trait SurfaceTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {
    fn pre_present_notify(&self, frame: Option<FrameStamp>, statistics: SwapChainStatistics);
    fn frame_pacing(&self) -> FramePacing { FramePacing::Display }
}

// flui-engine
pub(crate) fn present_mode(pacing: FramePacing, offered: &[wgpu::PresentMode]) -> wgpu::PresentMode;
```

- The Wayland host's surface target answers `Compositor`, whether its window is open or retired;
  every other host keeps the default.
- `Renderer::new` and `SurfaceFactory::create` take the window's `Arc<dyn SurfaceTarget>`; the
  surface lease records `frame_pacing()` when it is made and every configuration after (the
  renderer's build, recovery, recreation and colour change, the raster executor's window slot)
  reads it from the lease, so a reconfiguration after the window retired its target presents as
  before. `WindowTarget` stays the lease's crate-private handle bound (ADR-0063).
- A surface the display paces presents in `Fifo`, as ADR-0353 states. A compositor-paced surface
  presents in `Mailbox` where the surface offers it, else `Immediate`, else `Fifo`: Mesa offers both
  on Wayland, and wgpu's GL backend offers only `Fifo` on Linux.

### Consequences

- Good, because a window its compositor does not draw costs one frame per grace and never holds the
  owner thread; the application keeps answering input, timers and its other windows.
- Good, because a shown window's cadence does not change: the callback still holds it to the
  compositor's refresh, and under `Mailbox` the compositor shows each committed buffer whole.
- Bad, because a Wayland swapchain holds one image more under `Mailbox` than under `Fifo`.
- Bad, because the GL fallback (ADR-0244 §1) is not covered: wgpu's GL backend offers only
  `Fifo` on Linux and sets no swap interval, so a hidden window drawn there may still wait in
  `eglSwapBuffers` ([gaps](../gaps.md#no-present-mode-queue-depth-vrr-or-tearing-control)).
- Neutral, because `Immediate` is used only on a driver without `Mailbox`; a compositor still takes
  the committed buffer whole, so it does not tear on a composited desktop.

### Confirmation

- `flui-engine`'s `surface_pacing::tests::the_pacer_chooses_the_present_mode`: the mode for each
  pacing and each set of offered modes.
- `flui-app`'s `native_linux::an_app_whose_window_is_never_drawn_still_quits`: an animating
  application on the native Wayland host under the scripted compositor, which answers no frame
  callback, drawing through Mesa's lavapipe, asks to quit four seconds after it is ready; with
  every surface in `Fifo` it is still running when its budget ends.
- `flui-native`'s `wayland_scripted::a_window_the_compositor_never_draws_keeps_the_loop_turning`:
  the host asks a window its compositor never draws for one frame per grace, and each turn returns
  in its time.

## Pros and cons of the options

### 1. The pacer chooses the present mode

- Good, because it removes the only wait on the compositor from the present path.
- Good, because the host that arms the frame callback is the one that states the pacing, read
  once into the lease, so every configuration path agrees and no guess from a handle's kind is
  made.
- Bad, because `Renderer::new` no longer takes a bare raw-handle owner: a caller hands it a
  `SurfaceTarget`.

### 2. `Fifo` everywhere, no grace

- Good, because no present mode changes.
- Bad, because a present the swapchain skipped (an outdated surface, a zero size) commits nothing,
  so its callback never comes and the window would never draw again.

### 3. `Fifo` on a present thread

- Good, because the owner never waits.
- Bad, because the present thread waits instead, holding the frame's resources, and the next frame
  waits on it: the window still stops, and the raster thread of ADR-0305 is not built yet.

### 4. Require `wp_fifo_v1`

- Good, because the protocol releases the barrier for a surface the compositor does not draw.
- Bad, because no compositor before GNOME 48 and KWin 6.3 has it, and Linux's floor (ADR-0244 §3)
  names none of them.

## More information

Raven presents without driver vsync wherever the compositor paces by frame callbacks, for the same
reason (`platform.h:359-373`), and bounds its own frame wait at one second as FLUI's host does.
Revisit when every compositor of Linux's floor offers `wp_fifo_v1`: a `Fifo` present then waits on
a barrier the compositor releases, and `Mailbox` costs an image it no longer needs to.
