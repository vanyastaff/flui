# ADR-0274: A flush publisher writes to the storage its document acquired, and rebases only its own line

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0259](ADR-0259-close-guard-close-reasons-and-flush-registry.md) §5 only (the
  registry's storage, `publisher`, `committed`, `state`, `FlushHost::new`, and the "edits are one
  line" clause)
- **Related:** [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md) (a
  document acquires its storage as a capability an application may provide),
  [ADR-0152](ADR-0152-capability-seam-revised.md) (the capability seam)

## Context and problem statement

ADR-0259 §5 built the flush registry over one storage, the host's. A document reads through the
storage it acquires as `StorageCapability`, which an application may provide itself (an encrypted
store, say), so its writes went to the host's directory while its reads came from the provider: an
encrypted document's bytes were written in the clear, to the wrong place, and reported committed.

The same section rebased every edit of a publisher onto its last confirmed write, and refused in the
registry any other based edit whose base was not that write's version. A document that reloaded a
value another process stored, and edited it, therefore had its edit rebased onto its own older
write, and the storage refused it with `Conflict` until the document was reopened.

Which storage does a published value go to, and when is an edit rebased?

## Decision drivers

- A document's reads and writes reach one storage, the one it acquired.
- A value is a name in a storage: two storages, even of one type, never share a value.
- The storage's own comparison decides a based write; the registry adds no conflict of its own.
- A publisher's queued edits, all based on the version it loaded, still replace each other's bytes
  without conflicting.

## Considered options

1. **The publisher carries its document's storage; values are keyed by storage identity and name;
   an edit is rebased only when its base is the base of its publisher's last confirmed write.**
2. **The host builds its registry over whatever storage the capability registry resolves.**
3. **A new publisher per load,** keeping the registry's single storage.

## Decision

Chosen option: 1, because the document's own storage is the only one that is always right, and
the rebase rule then needs no knowledge of who else wrote.

```rust
// flui_view::__runtime
impl FlushRegistry {
    pub fn publisher(&self, name: StorageName, storage: Arc<dyn Storage>) -> FlushPublisher;
    pub fn changed(&self) -> FlushChanged;
}
impl FlushPublisher {
    pub fn name(&self) -> StorageName;
    pub fn committed(&self) -> Option<(Revision, StoredVersion)>;
    pub fn state(&self) -> FlushState;
    pub fn publish(&self, bytes: Arc<[u8]>, mode: WriteMode, revision: Revision);
}
impl FlushHost {
    pub fn new(writer: FlushWriter, notify: OwnerNotify) -> Self;
}
```

- **A value is a name in a storage.** The registry keys owed bytes and committed writes by the
  storage's `Arc` address and the name. Every record under a key holds that `Arc`, so no other
  storage can be given the address while the key is in the registry. The type of the storage plays
  no part.
- **Writes go to the publisher's storage.** `Persisted` makes its publisher with the store of the
  `StorageHandle` it acquired; the host's flush at exit and at a session end writes each owed value
  to its own storage.
- **A publisher's edits are one line.** A confirmed write records the base it was published with,
  before any rebase. An edit from the same publisher with that same base is rebased onto the
  confirmed write, whose bytes it replaces. Every other based edit is written as published, and the
  storage's compare-and-replace decides it: a base the storage no longer holds is
  `StorageError::Conflict`, from the storage.
- **The testing kit compares too.** `MemoryStorage` refuses an `IfUnchanged` write that commits
  over another version with `Conflict`, so a headless test sees what the file store does.

### Consequences

- Good, because a provided storage receives every write of the documents that acquired it.
- Good, because a document that reloads another writer's value writes over it without reopening.
- Bad, because a stale edit from another publisher reaches the storage before it is refused, one
  write call more than a refusal in the registry.
- Neutral, because the host no longer hands the registry a storage; it still provides the built-in
  one as the capability.

### Confirmation

- `flush_registry_matrix` (`flui-view` unit tests): `a_publishers_own_line_rebases_and_another_conflicts`,
  `same_writer_edits_rebase_instead_of_conflicting`, `a_reloaded_base_is_written_as_published`,
  `two_storages_of_one_type_keep_a_name_apart`.
- `crates/flui-view/tests/persist.rs`: `retry_after_failure_publishes_the_latest_value`,
  `a_later_edit_supersedes_an_unstarted_one`.

## Pros and cons of the options

### 1. The publisher carries its document's storage

- Good, because reads and writes cannot diverge: both use the handle the document holds.
- Good, because two runtimes with different providers of one name never share a value.
- Bad, because every record holds an `Arc` of its storage.

### 2. A registry over the resolved capability

- Good, because the registry keeps one storage.
- Bad, because a capability resolves per UI runtime, and one host registry serves several; the
  registry would still write some documents to the wrong storage.

### 3. A new publisher per load

- Good, because it is a change in `Persisted` alone.
- Bad, because the registry's own refusal still compares the reloaded base with the last confirmed
  write and refuses it, whichever publisher writes.
- Bad, because it leaves the override writing to the host's storage.

## More information

ADR-0259 §5's other clauses stand: the latest bytes win, a failed write keeps its bytes, the
registry is the one path to storage, and it is not a widget capability.
