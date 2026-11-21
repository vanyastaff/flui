### Removed

- `flui-view`: `WidgetsBinding::{first_frame_rasterized, mark_first_frame_rasterized, is_ready_to_produce_frames, mark_ready_to_produce_frames}`, flags nothing set or read; the first-frame gate is the renderer binding's (`UiRuntime::defer_first_frame`).
