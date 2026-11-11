### Added
- `flui_layer::RasterBase`: the raster side's half of the damage contract, which applies a publication's damage only to the image of its base version on a surface of the same size and draws in full otherwise, naming the reason.
- `SceneSnapshot::of_version`: a snapshot that carries the published scene version and its base beside the tree the engine draws.
- `flui_rendering::testing::SceneHost`: a retained scene a pipeline test drives its frames into, with `drive`, `apply`, `drive_cell`, `apply_cell` and `content_of`.
- `SceneWriter::discard`: a painter hands the scene a content it recorded for a patch that was never applied, and the scene drops it when the next patch opens, outside the painter's borrows.
- `FrameReport::scene` in `flui-testing`: the frame's scene operations, damage rectangles, full-damage frames, table copies and depth updates, counted by the perf baseline.

### Changed
- Paint writes a patch of the retained scene instead of building a `LayerTree`: a repainted repaint boundary is published as its own node, a clean one keeps its content and costs no operation, and a composited-layer update republishes its boundary without repainting it. A boundary holding a follower or a leader is retained like any other.
- `PipelineOwner::run_frame`, `run_frame_in_place`, `run_paint`, `PipelineCell::run_frame`, `RenderingBinding::draw_frame` and `BuildOwner::run_frame_with_layout_builders` take the `SceneWriter` the patch goes into and answer whether there is a frame to apply.
- `FrameSink::submit` takes a `ScenePublication`: the presentation's scene version and the damage since the version the raster side last showed. A frame the sink does not show leaves its damage owed to the next one.
- A hot-reload plugin frame is slotted by its scene's root, so the leaders it paints are attached for followers and leader watches.
- A hot reload or a refused patch renews the presentation's scene, and the next frame is drawn in full.
- `PipelineOwner::scene_applied` returns the version it replaces, to be dropped once no borrow of the owner is held: its annotations are user values.
- `SceneOwner::renew` runs the version sequence on unless it is spent, so an inspection handle kept from before a renewal names no layer after it.

### Removed
- `LayerDiffer`, `ContentToken` and `BoundaryStamp` from the production API: they remain behind `flui-layer`'s `testing` feature as the oracle the damage tests compare against.
