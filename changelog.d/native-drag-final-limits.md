### Fixed

- Drag requests now reject oversized declared representations before calling the source and enforce `max_bytes` on final ready and pending payloads from every transport. Payloads exactly at the limit remain valid, and source errors and cancellation are preserved.
