### Changed

- Synthetic accessibility identities now recycle their slots with checked generations instead of permanently spending a 32-bit monotonic pool. The published 64-bit IDs use a domain tag that prevents aliases with render-backed nodes; stale and double releases cannot return a slot twice.
- Packed generational IDs reserve their top bit for the domain tag, leaving 31 generation bits. A slot still closes permanently at its terminal generation.
- Text runs keep their synthetic IDs through same-count edits, including shifted starting bytes, so repeated edits no longer exhaust the identity source. A selection position is decoded only while the current and last delivered text coordinates agree; an undelivered edit is refused instead of redirecting an outstanding position to different bytes.
