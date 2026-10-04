### Changed

- Asset registry loads validate descriptors before cache lookup, including cache hits; rejected descriptors preserve accepted cached data ([ADR-0105](/docs/adr/ADR-0105-asset-validation-and-bridge-progress.md)).

### Fixed

- Bridged image loads progress while an ambient current-thread Tokio runtime is entered but undriven; typed fallback runtime creation errors allow later attempts to retry.
