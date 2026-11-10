### Changed

- **`ListenerId`, `FrameCallbackId` and `TaskId` are opaque minted identities** (`flui-foundation`):
  each is a private `NonZeroU64` whose only constructor is the `IdMint` family, so no caller can
  build one from a number and reissue an id a live registration holds. They are no longer
  `Id<Marker>` aliases: `zip`, `new`, `try_zip`, `from_raw` and the `Identifier` impl are gone, and
  `get()` returns the raw `u64` for diagnostics. They are 64-bit on every target, so a listener
  list on wasm32 no longer runs out at `u32::MAX` registrations. A listener ledger outside the
  framework mints from an `IdMint<ListenerId>` of its own.
- **Scheduler callback ids are minted owner-locally** (`flui-scheduler`): `UpdateScheduler`'s
  callback ids come from an `IdMint`, since only the owner thread registers callbacks; `TaskId`
  keeps the process's `AtomicIdMint`.
- **`TaskToken::id` returns `Option<NonZeroU64>`** (`flui-scheduler`): `None` for a task the driver
  refused, instead of the `0` sentinel. Async task ids come from the task store's own `IdMint`
  instead of a hand-written counter.
- **`TextureRegistry` listener keys come from an `IdMint`** (`flui-layer`); exhaustion still refuses
  `listen` for good.

### Removed

- **`flui_foundation::markers::{Listener, FrameCallback, Task}`**, the markers of the three ids
  above.
- **`flui_scheduler::{AtomicIdMint, Id, Marker, markers}`** and `flui_scheduler::id::Identifier`
  re-exports: nothing the scheduler issues is an `Id<Marker>` any more. Name the mint from
  `flui_foundation`.
