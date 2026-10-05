### Changed

- Asset keys own shared string names instead of process-global interned indices. Keys implement Clone without Copy, compare and hash by contents, and no longer expose as_u32 or a static string borrow.
- Font and image constructors accept Into<Arc<str>> and reuse shared name storage when producing keys. Borrowed String inputs use as_str().

### Fixed

- Asset name storage can be released after its final key owner disappears, including keys owned by weak data handles, instead of retaining every name for the process lifetime.
- Remove Lasso and the asset interner's process-global exception.
