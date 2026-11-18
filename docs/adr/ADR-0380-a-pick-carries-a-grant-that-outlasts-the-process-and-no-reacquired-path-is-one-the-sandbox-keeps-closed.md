# ADR-0380: A pick carries a grant that outlasts the process, and no reacquired path is one the sandbox keeps closed

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Related:** [ADR-0154](ADR-0154-capability-crates.md) §10 and §11 (the scenario primitive and
  `flui-system`), [ADR-0344](ADR-0344-a-dialog-lasts-while-its-caller-holds-it-and-ends-with-its-window.md)
  (a dialog's answer is one the request can have),
  [ADR-0360](ADR-0360-a-dialog-filter-means-one-thing-on-every-host-and-a-launch-names-the-schemes-it-allows.md)
  §2 (no file system call on the owner thread),
  [ADR-0377](ADR-0377-non-rust-code-on-android-and-apple-is-a-leaf-by-rule.md) (Android's
  classes are called from Rust),
  [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md)
  §1 (owner-thread state, and what crosses a thread names it)

## Context and problem statement

`flui-system`'s file dialogs answered what the user picked for the process's run alone. A
sandboxed process (an App Store build on macOS or iOS, a Flatpak, an Android application, an
application in an AppContainer) may open a picked file because its system granted it at the
pick, and only for as long as that grant lasts:

- macOS gives an App Sandbox's process access to a pick until it exits; reopening the file on
  the next run needs a security-scoped bookmark made while the access is held.
- iOS gives access only while the picker's security-scoped resource is started, so the provider
  read every pick there and kept nothing.
- Android hands a content URI whose permission lasts for the Activity unless the application
  persists it (`ContentResolver.takePersistableUriPermission`), within a bounded number Android
  trims the oldest of.
- A Flatpak's file chooser answers a path in the document portal's folder, which the portal
  keeps for the application across its runs; outside it the process sees only what its
  manifest exposes.

`data_dirs` assumed an unsandboxed process as well: a Flatpak with no `XDG_DATA_HOME` would be
answered `~/.local/share`, which its sandbox does not expose; a macOS App Sandbox whose `HOME`
was cleared the user's own home, outside its container; a packaged Windows application a folder
under `%APPDATA%` that Windows redirects into the package's private copy.

How does an application reach a picked file again after a restart, and how does the layer make
sure it never hands out a path the process cannot open?

## Decision drivers

- The rule the layer keeps is one rule on every host, held where the scripted double has it too
  (ADR-0154 §10): a scenario must be able to simulate a sandbox, a restart and a revocation.
- What an application stores is bytes, in its own settings or beside a document it remembers;
  an object of the system's (an `NSURL`, a `Uri`, a `FileSystemHandle`) does not survive its
  storage.
- A failure says what happened in a form a caller branches on: the user must pick again, the
  file is gone, the grant came from another kind of machine.
- No file system call on the owner thread (ADR-0360 §2); making and resolving a bookmark read
  the file system.
- Android caps persisted permissions per application, so keeping one is the application's
  choice, not every pick's.

## Considered options

1. **Paths only**, and a sandboxed host documents what it cannot do (the state before).
2. **A grant on the pick and a separate resolving service**, the handle holding every answer to
   the process's sandbox.
3. **Resolve through the file dialogs' handle**, a `FileDialogs::reopen` beside `open` and
   `save`.
4. **A resolved grant is an open file** (`std::fs::File`) on every host.

## Decision

Chosen option: 2, because it is the only one that gives a sandboxed process its file back and
puts the rule in one place: the handle, which every backend and the scripted double answer
through.

### §1. A pick carries a grant the application stores

`OpenRequest::keep_access` asks each pick to carry a `flui_system::grant::Grant`
(`PickedFile::grant`). A `Grant` is the system's lasting access in its own form, tagged by its
`GrantKind`: `AppleBookmark` (a security-scoped bookmark on macOS, a minimal bookmark made under
the picker's access on iOS), `ContentUri` (the URI whose read permission was persisted after
every pick was read, so an answer refused midway persists nothing), `UnixPath` and
`WindowsPath` (where the process opens what the user can, and a Flatpak's document portal
path), and `Scripted` (the double's). `Grant::to_bytes` writes a versioned header and the
payload; `Grant::try_from_bytes` reads it back or answers a typed `MalformedGrant`. A pick its
system could not grant lasting access to is handed over without one. A save carries none: its
location is reopened by an open dialog.

### §2. A separate service resolves a grant

`GrantCapability` hands out `Grants`, which is not bound to a window: `Grants::resolve(&grant,
max_bytes)` answers a `Completion<Result<Access, GrantError>>`. An `Access` is the file again
(`PickedFile`: a path, or on Android its name and contents read within `max_bytes`), a fresher
grant when the system called the old one stale (`Access::renewed`), and the `AccessScope` that
keeps it open while the `Access` is held: a macOS or iOS security-scoped resource, started where
the grant was resolved and stopped when the scope drops. `AccessScope` is `Send` because it is
started off the owner thread and crosses to the owner with the answer.

`GrantError` names what a caller branches on: `Foreign` (a grant of another kind of system, not
asked about), `Revoked` (the system no longer grants it, or resolved it to a path the process
may not open: pick again), `Missing` (deleted, or moved where the system does not follow),
`TooLarge`, `Unsupported` and `Failed`. A stale bookmark is not an error: it resolves and hands
a fresher grant over.

### §3. The handle holds every answer to the sandbox

`flui_system::sandbox::Sandbox` states what the process may open without a grant: everything, or
the folders of its sandbox (`SandboxKind::AppSandbox`, `IosApp`, `Flatpak`, `AndroidApp`), read
from the environment the system sets (`APP_SANDBOX_CONTAINER_ID` and the container's `HOME` and
`TMPDIR`, `FLATPAK_ID`), never from the file system. `Grants::new` takes the provider's sandbox.
The handle refuses a reacquired path the sandbox does not reach and no scope holds open
(`Revoked`), contents past `max_bytes` (`TooLarge`), and a folder handed over by its files, a
download or a fresher grant of another kind (`MalformedAnswer`), whichever backend gave it.

### §4. A process's data folders are its container's

`flui_native::storage::data_dirs` answers a sandboxed process its container: a Flatpak's
`$XDG_DATA_HOME`, else `~/.var/app/<id>/data`; the macOS App Sandbox's
`~/Library/Containers/<id>/Data/Library/Application Support`, from `HOME` or, without it, from
the user's home and the container id; a packaged Windows application's
`%LOCALAPPDATA%\Packages\<family name>\RoamingState` and `LocalState`. The Linux user folders
answer none for a folder that is not there for the process, which is how a Flatpak shows a
folder its manifest does not expose.

### Consequences

- Good, because an application in a store or a Flatpak reopens a document after a restart, and
  every way that fails is one variant a caller handles.
- Good, because the scripted double simulates the system's permission store across a restart
  (`testing::GrantStore`), a revocation and a sandbox, so the rule is tested on every host.
- Good, because a grant synced to another kind of machine is refused before any system call.
- Bad, because a Windows process in an AppContainer is not served: FLUI's window and dialogs are
  Win32's, the future-access list is not built, and its path grants answer `Revoked` when the
  file system refuses the path ([gaps](../gaps.md#sandbox-aware-file-access-is-partial)).
- Bad, because a page keeps no grant: a browser's lasting access is a `FileSystemHandle` stored
  as an object, which no provider keeps yet (`NotYetOnThisHost`).
- Bad, because the macOS bookmark needs the `com.apple.security.files.bookmarks.app-scope`
  entitlement, which `flui build` does not write yet; without it a pick carries no grant.
- Neutral, because `PickedFile` gains a field and `OpenRequest` a public one, before the first
  release.

### Confirmation

- The compiler: `Grants::new` takes a `Sandbox`; an `AccessScope` is `Send`; a `Grant` has no
  empty or oversized payload (`Grant::try_new`).
- Tests: `a_reacquired_path_is_handed_out_only_where_the_process_can_open_it`,
  `a_picks_access_survives_a_restart_through_the_bytes_the_application_kept` and
  `a_grant_the_system_no_longer_honours_answers_why` in `flui-system`'s `system_it`;
  `a_foreign_grant_is_refused_without_asking_the_system`, a row of
  `every_provider_keeps_the_system_services_contract`;
  `a_picks_path_is_its_grant_and_resolves_while_the_file_is_there` against the stand-in portal on
  Linux; `a_kept_pick_carries_its_uri_once_every_pick_was_read` and
  `a_document_is_reacquired_only_while_its_permission_is_held_and_within_the_bound` (Android, on
  every host); `a_bookmark_that_does_not_resolve_answers_why` and
  `each_pick_carries_the_bookmark_made_of_it_or_none` (Apple, on every host);
  `a_sandboxed_process_keeps_its_data_in_its_container` in `flui-native`.

## Pros and cons of the options

### 1. Paths only

- Good, because nothing changes.
- Bad, because a store or Flatpak application cannot reopen a document, and iOS cannot pick a
  folder at all.

### 2. A grant on the pick and a separate resolving service (chosen)

- Good, because resolving needs no window, and the rule lives in one handle.
- Bad, because a new capability is registered on every host.

### 3. Resolve through the file dialogs' handle

- Good, because no new capability.
- Bad, because `FileDialogs` is bound to a window and refuses a widget in none, while an
  application reopens its last document before any window shows; and its one-dialog-per-window
  rule has no meaning for a resolve.

### 4. A resolved grant is an open file

- Good, because the process opened it, so it can.
- Bad, because a folder is no `File`, a path is what a desktop application and its libraries
  take, and Android's documents would need a descriptor kept open for as long as the
  application holds the access.
