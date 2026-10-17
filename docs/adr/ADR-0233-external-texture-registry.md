# ADR-0233: External textures fed through a presentation registry

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** ADR-0078 (lifecycle capabilities), ADR-0115 (image source regions)

## Context

The engine could draw imported textures (`ExternalTextureRegistry`, `TextureLayer`,
`Canvas::draw_texture`), but nothing outside the engine could import one: `flui-app`
had no path to the renderer's registry, ids were chosen by callers, and nothing
repainted when only a texture's contents changed, because an unchanged UI submits
no scene.

## Decision

`flui_layer::TextureRegistry` belongs to one presentation and is reached by widgets
through `LifecycleContext::texture_registry`. `register()` issues a
`TextureRegistration` whose `TextureId` is never issued again; exhausting ids
refuses permanently. The registration is owned: `submit_frame(Image)` hands over the
next frame from any thread, and dropping it retires the id. Frames are CPU images
(straight-alpha, encoded-sRGB RGBA8); importing a GPU texture without a copy needs a
shared device and is not part of this contract.

Each frame bumps the id's revision, runs its listeners outside the registry's lock,
and wakes the runtime. The `Texture` widget listens to its id, rebuilds with the new
revision, and `RenderTexture` repaints on a revision change, so the frame reaches
the screen without anything else changing.

Before each frame renders, the raster lane (and the web runner's direct sink) takes
the registry's updates — the newest frame per texture, and the retired ids — and the
renderer applies them: a frame of the same size replaces the allocation in place, a
resized one re-registers it, and a retired id is unregistered. The renderer keeps the
newest frame of each live texture and uploads them again after rebuilding its painter
(device recovery, surface format change), which drops imported allocations.

## Verification

`texture_registry` (flui-layer) pins unique ids, newest-frame delivery, retirement and
reentrant listeners. `registry_frames_upload_replace_resize_and_retire` (flui-engine)
reads back each upload path and the retired id's refusal.
`texture_updates_reach_the_backend_before_their_frame` (flui-app) pins the lane's
ordering. The `external_textures` rows of `frame_pacing_and_pump_matrix`
(flui-runtime) pin a cross-thread frame repainting the widget and unmount retiring
the texture, and `harness_texture_fills_its_box_and_repaints_per_frame` pins the
render object. The re-upload after a device rebuild is not exercised by a test.
