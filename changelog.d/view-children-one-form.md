### Added

- A fixed-size array `[V; N]` is a `ViewSeq`, so children of one type pass as `Row::new([a, b])`.

### Removed

- The `column!` and `row!` macros. A multi-child widget takes its children directly as a tuple
  (`Column::new((a, b, c))`, `(a,)` for one child), an array or a `Vec`.
