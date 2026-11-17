### Changed

- A session end's release hook that panics while its last block is released outside an unwind
  passes the panic to whoever released the block, with the block already gone; it was discarded
  without a trace. During an unwind it is still discarded behind the panic unwinding.
- `OwnerCalls::resume` called while the thread is already unwinding discards the scope's failure
  behind the unwinding one instead of raising a second panic (ADR-0281 §4).
