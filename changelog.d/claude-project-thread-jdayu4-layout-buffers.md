### Changed

- Layout checks each node's child lists out of buffers its `PipelineOwner` keeps across frames instead of allocating them per node; a full reassemble and a list scroll each allocate 320 times fewer.
