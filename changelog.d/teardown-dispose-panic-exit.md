### Fixed

- A `ViewState::dispose` (or a pending future's destructor) that panics while a window closes is no
  longer lost: the rest of the tree still disposes, and the first such panic is the close's failure,
  so closing the last window that way exits with code 101 after the flush instead of 0
  (ADR-0258 §2).
