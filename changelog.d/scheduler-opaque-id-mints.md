### Added

- `AtomicIdMint` issues sealed 64-bit identities across threads with the same permanent-exhaustion contract as owner-local `IdMint`.

### Changed

- `TaskId` and `FrameCallbackId` are opaque `NonZeroU64` identities issued only by foundation mints; scheduler callback and task admission no longer maintains independent counter implementations.

### Removed

- Removed resettable `IdGenerator` and the duplicate `flui_foundation::FrameId`; frame identity remains `flui_scheduler::FrameId`.
- Removed public numeric constructors and conversions from task and frame-callback identities, preventing stale identity reissue.
