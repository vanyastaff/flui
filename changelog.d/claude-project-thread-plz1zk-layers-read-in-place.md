### Changed

- `inspect.layers` and `flui_testing::widgets::LaidOut::inspect_layers` read the retained scene
  in place instead of a layer tree flattened for each read. A layer id names a boundary
  (`l4.2@7`) or an entry of its content (`l4.2:3@7`) and keeps it across frames that leave the
  boundary in the scene; the agent protocol is 0.11.
- `PluginPipeline::draw_frame` hands the host the frame's retained version
  (`Scene::of_version`), which `HeadlessRenderer::render_scene` and the windowed renderer walk.

### Removed

- `SceneVersion::flatten` and `flatten_node` are test support, behind `flui-layer`'s `testing`
  feature, and the layer tree no longer implements `InspectTree`.
