# ADR-0391: Every native host drives the scene frame

- **Status:** Accepted (2026-11-20)
- **Date:** 2026-11-20
- **Supersedes:** [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) §4
  only for the `DevSceneHook` column of its table and the sentence that a desktop scene plugin runs
  through the package's own driver in `packages/flui-hot-reload/examples/scene_render.rs`: every
  native host asks the scene hook for each presentation's frame.
- **Related:** [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §10 (every window draws through a slot of one executor; a lent scene is drawn inline);
  [ADR-0108](ADR-0108-plugin-scene-render-callback-safety.md) (the scene callback's lifetime
  contract and its font reset); [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md)
  (the hook is user code, lent out of its slot); [ADR-0353](ADR-0353-production-is-paced-by-the-platform-and-a-frame-that-showed-nothing-waits-for-the-fallback.md)
  (the desktop's pacing fallback, which a presented frame stands down);
  [ADR-0389](ADR-0389-the-frame-a-covered-window-owes-its-surface-is-asked-of-that-window.md) (a
  covered window's owed frame, which stays a widget frame)

## Context and problem statement

A development scene plugin draws whole frames in place of the widget tree (`flui run --scene`,
`flui_hot_reload::ScenePluginHook`). Its host half is `ScenePlugin` in
`crates/flui-app/src/app/hot_reload.rs`, which lends the hook out of its slot and hands its scene
to the window's slot through `RasterHost::render_lent`. What `main` held:

| Fact | Where it shows |
|---|---|
| Only the Android runner asked the hook for a scene frame; the desktop and iOS runners drove `attach`, `poll` and `detach` alone, so an installed scene hook drew nothing there | `crates/flui-app/src/app/runner/android.rs`, `runner/desktop.rs`, `runner/ios.rs` |
| A desktop scene plugin ran through a host of its own, written on the public `flui_engine::Renderer`, which no other window path uses | `packages/flui-hot-reload/examples/scene_render.rs` |
| Every native window, the desktop's included, draws through a slot of the one raster executor, and `render_lent` is a call of that executor | `crates/flui-app/src/app/runner/raster_host.rs` (ADR-0305 §10) |
| When Android's plugin drew, the wake returned before its gate and ran no background pump, so the frame request it served stayed latched and a due input deadline or owner-local task waited for a widget frame | `runner/android.rs` before this record |

Which hosts ask a scene hook for a frame, and through which seam?

## Decision drivers

- One form of window rendering: after ADR-0305 every native window draws through a slot of one
  executor, and a second renderer kept alive for one development path is a replaced form left
  alive (AGENTS.md, "Recurring defects").
- One rule at the seam, not held by one host: when the plugin owns a frame, what that frame still
  owes (the frame request it served, the work the runtime accepted, the window's pacing and first
  reveal) is decided once, and every host follows it (AGENTS.md, "Before each push" item 7).
- The hook is user code: it is lent out of its slot with no lock or borrow held, and a panic
  costs hot reload, never the frame (ADR-0311 §5, ADR-0287).
- With no hook installed, a frame pays one `Option` check.
- Flutter has a single embedder path: frame scheduling and hot reload live in the shared engine
  shell, and a platform embedding supplies only its vsync and its surface, so no embedding has a
  development path of its own.

## Considered options

1. Every native host drives the scene frame through one seam in `flui-app`.
2. Keep Android alone, with a hand-written desktop host on the public `flui_engine::Renderer`.
3. A public embedder API on `GpuContext`, through which a tool draws a scene into a window itself.

## Decision

Chosen option: 1, because the executor every window already draws through can draw the plugin's
scene, and the rule for a frame the plugin owns is then written once.

§1. **Where the hook is asked.** The Windows, macOS, Linux, iOS and Android runners call
`ScenePlugin::own_frame(&self, ui_runtime: &UiRuntime, raster: &RasterSlot) ->
Option<PluginFrame>` at the start of each presentation's frame on the owner thread, inside the
runtime's entry, after the owner-inbox drain (and on the desktop after the development poll, the
compositor tick and a requested device retry) and before the frame's dirty gate. Every window a
runner draws asks, each at its own physical size: a multi-window desktop app shows the plugin's
scene in each content window. The desktop and iOS runners keep driving `attach`, `poll` and
`detach` as ADR-0311 §4 states; the Android runner drives `scene_frame` alone. The web runner
drives no hook.

§2. **When the hook is not asked.** `own_frame` answers `None`, and the widget frame runs as
without a hook, when no hook is installed (one `Option` check), when the presentation's frames are
off (its window cannot be seen, so its slot draws nothing), when the window's executor turns on a
raster thread (`RasterSlot::lends_scenes`: a lent scene is drawn within the call, which only an
inline executor can do; the Wayland desktop host, logged once per hook), and when the hook drew
nothing, its scene was not shown, or it panicked. A covered window's owed frame (ADR-0389) is a
widget frame.

§3. **What a frame the plugin owned still owes.** `Some(PluginFrame)` is `#[must_use]`, and the
runner hands it to `PluginFrame::finish(self, ui_runtime: &mut UiRuntime, raster: &RasterSlot)` in
place of the widget frame. `finish` runs the runtime's background pump, which consumes the frame
request the wake served and serves owner commands, ready tasks and due input deadlines, then the
window's report of a panic the lent render left; each is contained, and the first panic resumes
after both. The desktop also records the frame as presented for its pacing fallback and its first
reveal (`DesktopFrameDriver::after_presented`), so a window that opens hidden for its first frame
is revealed by the plugin's, and an armed pacing deadline does not wake the loop again. The widget
tree's own visual work (builds, layout, animation frames) waits while the plugin owns the
window's frames: nothing asks a frame for it, so nothing spins.

§4. **Containment.** The hook is lent out of its slot for the call and no lock is held
(`DevReload::lend`); a panic disables the hook for the rest of the run and the wake's widget frame
runs (ADR-0311 §5). The render callback shows the window and lends the scene to its slot
(`RasterSlot::render_lent`), which keeps no reference past the call (ADR-0108).

### Consequences

- Good, because a desktop or iOS app with `ScenePluginHook` installed shows the plugin's scene
  in its own windows, and the package needs no host of its own: the hand-written desktop host and
  its use of `flui_engine::Renderer` can go.
- Good, because the work a plugin-owned frame owes is decided in `hot_reload.rs` once; Android's
  frame now serves its background work as the desktop's and iOS's do.
- Bad, because on the Wayland desktop host, whose executor turns on a raster thread, a scene
  plugin draws nothing until a lent scene can be drawn on that thread (`docs/gaps.md`).
- Bad, because a worker hook's frame pays one lend of the hook per presentation frame for a
  `scene_frame` that draws nothing.
- Neutral, because the hook is asked once per presentation frame, so a multi-window app's plugin
  builds one scene per window.

### Confirmation

- `flui-app`'s `a_desktop_window_draws_its_scene_plugins_frame_in_place_of_the_widget_frame`, a
  row of `installed_frame_driver_contract`, drives the desktop frame driver over scripted slots:
  a hook that draws is asked at the window's size, its scene is lent to the slot, no widget frame
  renders, the frame request is consumed and the window is revealed; a hook that draws nothing or
  panics leaves the widget frame; a hidden window and a window on a raster thread do not ask.
- The `#[must_use]` on `PluginFrame` makes an answer a runner discards a warning, which clippy's
  `-D warnings` refuses; the test above fails when the desktop drops the answer instead of
  finishing it (the frame request stays scheduled).
- Android and iOS share `own_frame` and `finish` with the desktop; they are type-checked for
  their targets on main, and the plugin's frame on a device is a run owed there.

## Pros and cons of the options

### 1. Every native host drives the scene frame (chosen)

- Good, because the plugin draws through the slot every window draws through, with the window's
  surface, device recovery and colour state.
- Good, because the frame's obligations are one rule at the seam, every host included.
- Bad, because a threaded executor cannot draw a lent scene, so the Wayland host is left out
  until it can.

### 2. Android alone, with a hand-written desktop host

- Good, because nothing in the runners changes.
- Bad, because the desktop host keeps a second renderer alive (`flui_engine::Renderer`) beside the
  executor, with its own surface handling, and the next development path copies it.
- Bad, because the rule for a plugin-owned frame is held by one host, and Android's dropped its
  background work.

### 3. A public embedder API on `GpuContext`

- Good, because any tool could draw into a window without the hook.
- Bad, because it publishes the executor's slots and their lifetime as API before the first
  release, for one development tool.
- Bad, because the tool would then own frame pacing, surface loss and the first reveal itself,
  which every runner already owns.
