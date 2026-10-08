### Changed

- Consolidated pointer identity and timestamp queries on the sealed `PointerEventExt` trait. Use `pointer_id()` and `time()`; device-only events keep no contact identity and timestamp zero remains valid.
- Removed the unused cached hit-test getter and restricted pointer metadata and sample helpers to their internal consumers.
