### Added

- **`PipelineCell::run_frame`** runs one frame of the owner behind the cell and puts the owner back,
  on the success and the error path alike.

### Removed

- `BuildOwner::run_settled_frame` and `WidgetsBinding::run_settled_frame`: they only forwarded to
  the pipeline. A frame that settled its layout with `settle_layout_with_builders` paints with
  `PipelineCell::run_frame`.
