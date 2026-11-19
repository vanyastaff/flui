### Removed

- `flui_layer::CanvasLayer`, `TextureLayer` and `PlatformViewLayer`, with `PlatformViewId`,
  `PlatformViewHitTestBehavior` and `SceneBuilder::add_canvas`. A drawing is a `PictureLayer`
  over the `DisplayList` a `flui_painting::Canvas` finishes, and an external texture is a
  `Canvas::draw_texture` call inside one (ADR-0233), so it is clipped, transformed and cached
  with the chunk that draws it. A native view inside the UI has no scene form yet
  (`docs/gaps.md`, "No native views inside the UI").
