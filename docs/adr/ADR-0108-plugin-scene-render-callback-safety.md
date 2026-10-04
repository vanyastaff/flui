# ADR-0108: Plugin scenes declare lifetime and font identity boundaries

- **Status:** Accepted
- **Date:** 2026-10-03
- **Supersedes:** ADR-0094's implemented scene callback safety claim; its driver
  topology and proposed Subsecond integration remain unchanged.

## Context

A plugin scene can contain annotated layers holding `Arc<dyn Any + Send + Sync>`
whose destructor vtable belongs to the loaded image. Lending `&Scene` to a safe
callback does not prevent that callback from cloning a layer or its payload.
It can retain the clone across a subsequent reload and execute unmapped code
when it eventually uses or drops that value. The old safe `scene_frame` call
therefore hid an obligation its signature did not establish.

The driver also constructed and returned a scene from safe `poll`. Both current
callers discarded that scene before separately building the frame. Polling the
artifact does not require scene construction.

## Decision

`HotReloadDriver::poll` updates the image and reports successful loading with a
boolean. Scene construction remains the separate unsafe `build_scene` operation.

`DevReloadHook::scene_frame` is unsafe. Its caller must ensure the render callback
retains no image-dependent scene data beyond the call. The contract covers
cloned layers and opaque payloads, including on unwind;
the borrowed scene reference alone does not discharge it. `ScenePluginHook`
implements that contract through the SDK seam and drops its local scene before
returning. No layer or scene is made non-cloneable merely to hide the escape.

The Android host supplies only the synchronous `Renderer::render_plugin_scene` callback.
That engine path ignores annotation payloads and records concrete geometry,
matrices, instances and GPU leases. Its texture cache may retain concrete `Image`
allocations backed by host-compatible byte vectors; these do not carry a plugin's
opaque vtable. Plugin font registry admission must copy font bytes into concrete
owned storage while the source image is live: an erased `FontBlob` can carry a plugin
vtable or a borrowed slice into its read-only data even for bundled fonts. The
existing loader ABI handshake remains required for shared Rust values.

Font blob IDs come from an image-local counter. Copying bytes establishes
ownership but does not distinguish two images that issue the same face ID for
different fonts. A scene hook therefore starts with a pending font reset and
requests another after every successfully loaded replacement image. The
callback receives that request and acknowledges it only after rendering
succeeds. Refusal, an unavailable scene, or unwind preserves the request.

The renderer distinguishes ordinary and plugin font sources. It replaces the
whole text atlas, including the rasterizer's registry and cached glyph bitmaps,
when changing sources or receiving a reset request. This happens between
frames, before damage planning or command recording. Both managed ordinary
frames and plugin frames use the same source selector; a transition owes full
repaint even when the scene differ reports no damage. Submitted GPU commands
own their previous resources, so replacing the atlas requires no GPU wait.
Warm frames within one image keep their atlas and perform no font copy or hash
merely to establish image identity. Ordinary registries retain the original shared
font source, preserving the weak-cache blob identity contract in ADR-0092 §5.
Only the plugin atlas uses `SwashRasterizer::with_owned_fonts`, which constructs
`FontRegistry::with_owned_sources`; returning to ordinary scenes restores the
shared-source policy alongside the atlas.

## Evidence and limits

The `scene_frame` compile-fail doctest rejects a safe caller, including through a
trait object. `scene_frame_default_never_calls_render` and the absent-plugin hook
test retain their existing behavior with explicitly justified unsafe calls.
`polling_an_unavailable_plugin_reports_no_change` pins the safe polling result.

`registered_fonts_release_the_source_and_keep_rasterizing` checks source
retirement and subsequent glyph pixels in the explicit owning registry.
`a_held_blob_keeps_its_keys_across_a_prune` preserves the ordinary registry
contract: its shared source keeps the shaper cache's blob ID alive. `scene_frame_reset_recovery_matrix`
checks the actual hook's first-image, replacement, refusal and panic paths.
Three rows of `parley_runs_read_back` render different fonts under identical
face/glyph keys, start from `NoDamage`, and compare the transitioned frame with
a fresh capture. Their ID remapping lives only in painting's testing support:
the production API deliberately cannot inject an image-local blob counter.
These headless rows exercise the shared source selector; windowed entry-point
wiring is inspected separately and still needs native smoke coverage.

Renderer inspection covers the Android host callback, `SwapchainFrame` scene
borrowing, the borrowed layer walk, the annotation branch, draw segments and
texture-cache image keys. A future renderer that retains opaque plugin values
must revise its ownership and this safety proof before calling the hook. Native
Android reload execution is not verified on the Windows development host.

This is an explicit boundary contract, not automatic image leasing. Arbitrary
third-party callbacks must establish the same lifetime ordering themselves.
