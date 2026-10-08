### Changed

- Resampler delivery reuses measured and predicted history storage when the event timestamp does not need to be raised.
- Saturated resampling bounds already-checked measured history in place while retaining the latest predictions and full pointer metadata.
- Resampler coalescing transfers the retiring packet's measured history storage after full pointer identity validation; overlapping timestamps retain canonical ordering and filtering.
- Least-squares computation keeps its bounded scratch frame separate from estimator selection and memoized queries.
