### Changed

- `HitTestResult::with_paint_transform` returns `Option<R>` and skips descendant traversal when no finite computed inverse is admitted. Successful scopes continue to restore transforms on return and unwind.
