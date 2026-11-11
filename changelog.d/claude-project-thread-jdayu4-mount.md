### Changed

- Mounting allocates less: an element outside every inherited provider no longer allocates an
  empty provider map, and a reconcile of up to eight children keeps its working lists on the
  stack. A full reassemble of the perf scenario makes 2 913 allocations instead of 3 474.
