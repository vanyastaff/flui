### Fixed

- A frame that panics in the shared GPU context now costs only the caches every slot shares (glyph atlas, image and path caches, scratch pools). The pipeline library and the presentation blit pipelines survive, so the next frame of every slot builds no pipeline again.
- Stopping the raster executor waits for every lifecycle send already under way. A command a send accepted is answered when the queue is drained, and a later send gets `RasterSendError::Gone`.
