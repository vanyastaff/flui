### Changed
- Async tasks retain one waker across polls, removing steady-state per-poll allocations.
- Cached GPU path geometry streams into draw segments without an intermediate vertex buffer.
- Foundational dependencies enable serialization and GPU conversion features only where needed.

### Fixed
- Coalesced scheduler and async wakes retry unpaid delivery after hook failures or installation, without an older successful callback erasing newer demand.
- The offscreen texture pool learns resized effect dimensions instead of repeatedly allocating behind a full inventory of obsolete textures.
