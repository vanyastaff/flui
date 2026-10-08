### Changed

- Resampler delivery reuses measured and predicted history storage when the event timestamp does not need to be raised.
- Saturated resampling bounds already-checked measured history in place while retaining the latest predictions and full pointer metadata.
