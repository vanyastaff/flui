# ADR-0262: Application state persists as versioned documents the application encodes, written through the host's flush registry over a byte-storage capability

- **Status:** Proposed. The contract is in place and not wired: `Storage`, `StorageName`,
  `WriteMode`, `StoredVersion`, `Stored` and `StorageError` (`crates/flui-platform/src/storage.rs`),
  `FileStore` (`crates/flui-native/src/storage/file_store.rs`), `Document`, `DecodeError`,
  `Revision`, `PersistError` with `CodecStep`, `SaveStatus` and `Persisted`
  (`crates/flui-view/src/persist/`), `AppConfig::with_storage_dir`, and `Router::from_stack` with
  `RouterHandle::stack`, which carry the top route only. `Persisted::load` resolves to
  `StorageError::Unsupported`, `set` refuses every value, and storage still reaches a UI runtime
  through `LifecycleContext::storage`.
- **Date:** 2026-10-28
- **Related:** [ADR-0259](ADR-0259-close-guard-close-reasons-and-flush-registry.md) (the flush
  registry every write goes through, and the close guard a document holds),
  [ADR-0260](ADR-0260-session-end-flushes-before-it-tears-down.md) (the registry is written inline
  at a session end), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) (the exit
  flush), [ADR-0152](ADR-0152-capability-seam-revised.md) (storage becomes a built-in capability and
  `LifecycleContext::storage` is removed), [ADR-0127](ADR-0127-exceptional-path-retention.md) (a
  codec panic's payload is retained), [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md)
  §2 (no `serde` in a Stable signature), [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no
  `static`), [ADR-0093](ADR-0093-router-is-the-primary-navigation-api.md) step 8 (stack
  restoration, decided here; web history stays there)
- **Sources:** `rust-lang/rust@41089d07a0a2afc16adfe28a760d94d73d7298a4`
  `library/std/src/sys/fs/windows.rs` (`rename`: `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`,
  then `FileRenameInfoEx` with POSIX semantics);
  `Stebalien/tempfile@303aaa99f2e2a03ac48643c0486c709d5081d746`, module `file::imp::windows`
  (`persist` passes a path without the `\\?\` prefix);
  `flutter/flutter@97cd226ec5ee4c4075b8516ea294152dba1fbfcc`
  `packages/flutter/lib/src/services/restoration.dart` (restoration keyed by widget identity)

## Context and problem statement

An application that keeps a list of notes and the screen the user was on needs both back after a
restart, a crash, a log-off, a full disk or a second running instance. Without framework support
each application writes its own versioned envelope, atomic replacement, retry, conflict detection
between instances, close holds and panic isolation of its codec: several hundred lines of
concurrent code, repeated per application and tested per application.

The framework cannot own the format either: a codec in the framework puts `serde` in Stable
signatures (ADR-0089 §2) and in every application's graph.

What does the framework store, what does the application encode, and on which path do the bytes
reach the disk?

## Decision drivers

- the application owns its data format and its migrations; no serialization crate in the
  framework's signatures or graph;
- a write survives a crash of the process: the file on disk is the old whole state or the new one,
  never a mix;
- only bytes and versions cross the IO boundary; the value, the codec and the status stay on the
  owner thread, `!Send`;
- one path to storage, the host's flush registry (ADR-0259 §5), so exit and session end write what
  was published without the UI runtime;
- a second instance never silently loses the first one's update;
- a panic in the application's codec costs one failed save, not the document or the process;
- no `static`, no lock held across application code, no IO on the frame path.

## Considered options

1. Capabilities only: a byte store with atomic write and compare-and-swap, and stack restoration;
   the envelope, ordering, retry and close holds are the application's.
2. Capabilities plus an owner-local versioned document, `Persisted<D>`: the application encodes the
   body, the framework frames it, writes it, retries it, holds the close and contains the codec.
3. A restoration registry keyed by widget identity, restored at mount.

## Decision

Chosen option: 2, because the hard part is written and tested once while the format stays the
application's.

### §1. A byte store is the capability

```rust
// flui-platform
pub trait Storage: Send + Sync + 'static {
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored>;
    fn publish(&self, name: &StorageName, bytes: Vec<u8>, mode: WriteMode) -> StorageFuture<StoredVersion>;
}
pub enum WriteMode { Replace, IfUnchanged(StoredVersion) }
pub struct StorageName(/* name + scope */); // [a-z0-9][a-z0-9_-]{0,63}, no Windows device name
impl StorageName {
    pub const fn from_static(name: &'static str) -> Self;   // roaming data
    pub const fn machine_local(name: &'static str) -> Self; // this machine and instance
}
#[non_exhaustive] pub enum StorageError {
    Unsupported(Unsupported), Inaccessible { kind: std::io::ErrorKind }, Busy, Full,
    TooLarge { len: u64, limit: u64 }, Conflict, Superseded, Cancelled,
}
```

`publish` accepts the bytes before it returns; a previous value of the same name not yet started is
replaced and resolves `Superseded`. A bad name in a constant is a compile error. Storage reaches a
UI runtime as a built-in capability of the seam (ADR-0152), and `LifecycleContext::storage` is
removed; without a configured directory (`AppConfig::with_storage_dir`) or on a platform without
files, the capability is absent and a document is `Unavailable`.

### §2. A write replaces the file atomically, and a version guards data against another instance

`FileStore` writes a unique temporary file in the target's directory, `sync_all`s it and renames
it over the target with `std::fs::rename`, then syncs the directory on Unix. On Windows `rename` is
`MoveFileExW(MOVEFILE_REPLACE_EXISTING)` with a POSIX-semantics fallback and a long-path-safe path;
`tempfile`'s `persist` is not used, because it passes a path without the `\\?\` prefix and fails
past 260 characters. A file is therefore the old whole state or the new one after a crash of the
process; without `MOVEFILE_WRITE_THROUGH`, a loss of power after "Saved" may bring back the previous
whole state, never a mix.

`IfUnchanged(base)` takes a short lock on the directory (`.flui-storage.lock`), compares the
stored version with `base`, and refuses with `Conflict` when another writer replaced it. Where the
directory cannot be locked (`ErrorKind::Unsupported`, network shares), the document is read-only:
an explicit refusal is better than an update lost to another instance. Data is written with
`IfUnchanged`, per-instance session state (`machine_local`) with `Replace`.

### §3. A document is the application's codec in the framework's envelope

```rust
// flui-view::persist
pub trait Document: Sized + 'static {
    const NAME: StorageName;
    const VERSION: u32;                        // from 1
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    fn initial() -> Self;
    fn encode(&self) -> Vec<u8>;
    fn decode(version: u32, body: &[u8]) -> Result<Self, DecodeError>;
}
pub struct Persisted<D: Document> { /* Rc: owner-local, !Send, Clone */ }
impl<D: Document> Persisted<D> {
    pub fn open(cx: &dyn LifecycleContext) -> Self;
    pub fn load(&self) -> impl Future<Output = Result<Rc<D>, PersistError>> + 'static;
    pub fn set(&self, value: D) -> Result<Revision, PersistError>;
    pub fn committed(&self) -> Option<Revision>;
    pub fn status(&self) -> SaveStatus;
    pub fn retry(&self);
    pub fn start_over(&self);
}
```

The framework writes a one-line header, `flui-document <name> <version> <revision>\n`, before the
application's body: the name guards against reading another document's file, the version selects
the decoder, and the `Revision` counts the writes across runs (`u64::MAX` makes the document
`Exhausted` for good). A file without the header or with another name is `Corrupt`; a version above
`D::VERSION` is read-only, never written; a body `decode` refuses is `Decode`, and the file is not
touched. `start_over` copies the bytes to the first free `<name>-corrupt-N` before writing
`initial()`. A load applies only its latest attempt.

`set` refuses until a load succeeded (there is no base to write against), encodes on the owner
thread with no borrow of the document held, and publishes the bytes to the flush registry
(ADR-0259 §5): there is no write loop of its own. `committed` and "Saved" follow the registry's
confirmation, after the rename. `Busy` is retried on the UI runtime's clock, at 50, 100, 200, 400
and 800 ms, then `Failed`; a hidden or paused application and a close request retry at once.

### §4. A codec panic fails one save and keeps the document

```rust
#[non_exhaustive]
pub enum PersistError {
    // …
    Panicked { during: CodecStep, message: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecStep { Encode, Decode }
```

`encode` and `decode` run under `catch_unwind`, each with no borrow of the document held:

- a panic in `encode` publishes nothing; the value in memory and the file on disk are untouched,
  the status becomes `Failed(Panicked { during: Encode, .. })`, and the next `set` works;
- a panic in `decode` leaves the file untouched and the document not loaded,
  `NotLoaded(Some(Panicked { during: Decode, .. }))`; `load` may be called again;
- the first failure stays in the status until the next success; later ones are logged;
- the message is read with `flui_foundation::panic::payload_text`, and the payload is retained with
  `retain_opaque_payload` rather than dropped, since its destructor is application code
  (ADR-0127 §2).

A value of `D` whose own drop panics twice before the boundary aborts, the limit ADR-0127 states.
The codec is the application's, so its panic is an application failure the document reports, not
an owner-operation failure: it reaches the application through the status, not through
`CallbackPanic` (ADR-0258 §1), and it never makes the process exit.

### §5. A document holds the close while its edits are not stored

Through the close guard of ADR-0259: `hold()` while a write is in flight (`Saving`), which refuses
the user's and the program's close and lets a session end through, since the registry writes the
published bytes then; `require_decision()` while the document is `Failed`, read-only or not loaded
with edits, which refuses every close the platform lets the application refuse. An `Unavailable`
document holds nothing. Where the platform cannot wait (the web, Android, iOS), what changed after
the last confirmed write is lost if the process is killed before the application is hidden.

### §6. The router's stack is restorable

`Router::from_stack(stack: Vec<R>, page) -> Result<Router<R>, RouterError>` opens on the whole
stack, bottom to top, and `RouterHandle::stack(&self) -> Vec<R>` reads it as every committed edit
left it; an empty stack is `RouterError::EmptyStack`. The application stores the stack as paths
(`Routable::to_path`) in its own document and parses them back with `from_path`, falling back to
its home route when a path no longer parses or names an item that is gone. This decides the stack
restoration of ADR-0093 step 8; web history, query and fragment stay there.

### Consequences

- Good, because an application writes a codec and a schema, not a storage engine.
- Good, because no serialization crate is in the framework's graph or signatures; the application
  depends on its own.
- Good, because what was published is written at exit and at a session end without the UI runtime
  (ADR-0258, ADR-0260).
- Good, because a second instance gets `Conflict` instead of losing an update, and a share without
  locks is read-only instead of racy.
- Bad, because "Saved" survives a crash of the process, not a loss of power.
- Bad, because the application maintains its own migrations in `decode(version, …)`.
- Neutral, because a codec panic is reported in the document's status, not as a `CallbackPanic`.

### Confirmation

- `crates/flui-view/tests/persist.rs`: `a_loaded_document_round_trips_through_memory_storage`,
  `saved_status_appears_only_after_commit`, `codec_panic_is_contained_first_failure_kept`,
  `no_write_without_a_loaded_base`, `newer_version_is_never_written`,
  `older_version_decodes_or_reports_without_writing`, `oversized_file_is_refused_unread`,
  `busy_retry_follows_the_realm_clock`, `a_later_edit_supersedes_an_unstarted_one`,
  `user_close_waits_for_an_in_flight_save`, `a_saving_document_holds_user_close_only`,
  `two_realms_keep_independent_documents`.
- `FileStore`'s own tests: the atomic replacement, the lock and `Conflict`, the classification of
  OS errors.
- `crates/flui-widgets/tests/contracts.rs`: `from_stack_restores_back_order`,
  `stack_reads_every_committed_edit`, `empty_stack_is_refused`.
- The acceptance app's restart and storage-failure tables through the facade
  (`acceptance_app_runs_through_the_facade`), with `serde_json` a dependency of the consumer only;
  `cargo heli xtask facade-combos` keeps `serde_json` out of the framework's graph.
- A dated native run through flui-driver's `os` lane: a kill in a write loop, a read-only file, a
  long and redirected profile path, two instances, a session end during a save.

## Pros and cons of the options

### 1. Capabilities only

- Good, because the framework promises the least.
- Bad, because every application repeats the envelope, the retry, the conflict handling, the close
  holds and the codec's panic isolation, and gets them wrong in its own way.

### 2. Capabilities plus `Persisted<D>`

- Good, because the concurrent part is written and tested once.
- Good, because the format stays the application's, so ADR-0089 §2 is not widened.
- Bad, because the framework owns more public types (13) that carry the Stable promise.

### 3. A restoration registry by widget identity

Flutter's `RestorationManager` restores state into widgets as they mount, keyed by restoration
IDs.

- Good, because state comes back without application wiring per screen.
- Bad, because restoring at mount cannot wait for the data a session depends on (a draft belongs
  to a note that must be loaded first), and the keys are string labels where FLUI compares typed
  identity.
- Bad, because data that must survive more than a session (the notes themselves) is not covered.

## More information

Revisit §2's durability if a requirement asks "Saved" to survive a loss of power
(`MOVEFILE_WRITE_THROUGH`, `F_FULLFSYNC`), and §5 if a platform gains a close that can wait for
some work and not other.
