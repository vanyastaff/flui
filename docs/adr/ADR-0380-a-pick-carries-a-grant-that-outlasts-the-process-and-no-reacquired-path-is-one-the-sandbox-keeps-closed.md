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
  keeps for the application across its runs, or the real path of a file the manifest's
  `filesystems` expose (`home`, `host`, an `xdg-*` folder); the process sees its own folders and
  those, and nothing else. Its description, `/.flatpak-info`, is the only sign it runs in one:
  a host process started from a Flatpak's terminal inherits `FLATPAK_ID`.

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
every pick was read, so an answer refused midway persists nothing, and given back when no grant
can hold the URI), `UnixPath` and `WindowsPath` (where the process opens what the user can, and
a Flatpak's document portal path). `Grant::to_bytes` writes a versioned header and the payload;
`Grant::try_from_bytes` reads it back, judging the payload's length before it copies a byte, or
answers a typed `MalformedGrant`. A pick its system could not grant lasting access to is handed
over without one. A save carries none: its location is reopened by an open dialog.
`Grant::kind` always names a system. The scripted double's grants exist only in a build with the
double (`test` or the `testing` feature): each stands in for the host's kind of grant and is
written with a tag outside every `GrantKind` followed by that kind, which a build without the
double refuses as `MalformedGrant::Kind`. No system's backend takes one (`Grant::made_by` is
false for it), so every one answers it `Foreign`; a backend says which grants it resolves
through `GrantBackend::takes`. The public surface is the same with and without the feature, so
enabling it in a test build changes no caller's types.

### §2. A separate service resolves a grant

`GrantCapability` hands out `Grants`, which is not bound to a window: `Grants::resolve(&grant,
max_bytes)` answers a `Resolving`, which lasts while its caller holds it. Dropping it before the
answer delivers nothing; a resolve still waiting for the sandbox leaves the queue, and one the
backend runs drops the `ResolveHold` it returned, which stops what can be stopped: on Android the
read stops before its next chunk and its cancellation signal is cancelled, so the worker's place
comes back, and a read that finished cancels nothing; on macOS and iOS the resolution under way
finishes but starts no security scope; on Linux and Windows the one `metadata` call finishes and
its answer is dropped. An `Access` is the file again (`PickedFile`: a path, or on Android its name and contents
read within `max_bytes`), a fresher grant when the system called the old one stale
(`Access::renewed`), and, for this crate's own providers, the `AccessScope` that keeps it open
while the `Access` is held: a macOS or iOS security-scoped resource, started where the grant was
resolved and stopped when the scope drops. Only the crate makes a scope (`AccessScope::new` is
crate-private; a backend outside it builds `Access::new`, which holds none), so a scope that lets
a path past the sandbox's rule is always a system's started access. `AccessScope` is `Send`
because it is started off the owner thread and crosses to the owner with the answer.

`GrantError` names what a caller branches on: `Foreign` (a grant of another kind of system, not
asked about), `Revoked` (the system no longer grants it, or resolved it to a path the process
may not open: pick again), `Missing` (deleted, or moved where the system does not follow),
`NotLocal` (a path grant naming a share, a pipe or a device, refused by the ADR-0360 §2 rule
before any file system call, as a relative path is), `TooLarge`, `Unsupported` and `Failed`. A
stale bookmark is not an error: it resolves and hands a fresher grant over.

### §3. The handle holds every answer to the sandbox

`flui_platform::sandbox` detects the sandbox once for every crate that needs it
(`Confinement::detect`, `Confinement::of_process`). A Flatpak is a Linux process with
`/.flatpak-info`, read off the owner thread, size-checked before it is read and bounded, and
never read on another system (where that name is a drive's root); only the file's absence says
the process is no Flatpak, and a description that could not be read is no answer, which no
caller keeps. Every crate asks through that module (`read_flatpak_info`, `is_flatpak`) and none
opens the file itself. Its `[Context] filesystems=`
entries are modelled as Flatpak writes them, flattened: the key file's escapes undone
(`\;` stays inside its entry), the access mode dropped, `home`, `host`, `host-os` (`/run/host`),
`host-etc` (`/run/host/etc`), absolute and `~/` paths, `xdg-run`, the `xdg-config`-style home
folders, and an `xdg-*` user folder only where the user's `user-dirs.dirs` configures it, since
Flatpak ignores one it does not. Flatpak unsets `TMPDIR`, so the variable is never read there; a
Flatpak reaches its private `/tmp`. A path a manifest hides with `--nofilesystem` under an exposed
folder is a tmpfs the file does not show: it is reached here, and a grant to it resolves
`Missing` rather than `Revoked`, the one limit the description leaves. The macOS App Sandbox is
`APP_SANDBOX_CONTAINER_ID`; its container is `HOME` only when `HOME` is that container's path
(`…/Library/Containers/<id>/Data`), else the container under the user's home as the system's
user database records it (`getpwuid_r`, never `HOME`). A `TMPDIR` counts
only when it lies inside the container (App Sandbox, iOS). `flui_system::sandbox::Sandbox`
states, from that, what the process may open without a grant: everything, or the folders of its
sandbox (`SandboxKind::AppSandbox`, `IosApp`, `Flatpak`, `AndroidApp`). A backend reads it
through `GrantBackend::sandbox`, off the owner thread where it reads a file, one read at a time;
each resolve waits for it, at most `Grants::MAX_WAITING` at once (one more is refused
`Unavailable`), and a read the backend did not answer, or panicked asking for, is asked again at
the next resolve: a panic leaves the slot unread, every resolve waiting for it is refused
`Unavailable`, and the panic is resumed after that (ADR-0287). The
handle refuses a reacquired path the sandbox does not reach and no scope holds open
(`Revoked`), contents past `max_bytes` (`TooLarge`), and a folder handed over by its files, a
download or a fresher grant of another kind (`MalformedAnswer`), whichever backend gave it.

### §4. A process's data folders are its container's

`flui_native::storage::data_dirs` answers a sandboxed process its container, from the same
`flui_platform::sandbox` detection: a Flatpak's `$XDG_DATA_HOME`, else `~/.var/app/<id>/data`;
the macOS App Sandbox's `~/Library/Containers/<id>/Data/Library/Application Support`; a packaged
Windows application's `%LOCALAPPDATA%\Packages\<family name>\RoamingState` and `LocalState`. A
host's store (`DataStore::of_process`) finds them at its first call that can read the
container, on the thread the host runs the call on, never the owner thread nor a shared IO
worker; one find runs at a time, and the calls that arrive meanwhile share its outcome, a
refusal included. A packaged application whose new root
holds no store and whose redirected `%APPDATA%` location holds one has it copied over once,
within a bound (`MAX_MIGRATED_FILES`, `MAX_MIGRATED_BYTES`), into a staging folder beside the
root, made with `create_dir` (one already at the name is an error, never a shared folder) and
renamed into place whole. One instance migrates at a time: the sweep, the copy and the rename run
under an exclusive lock (`File::lock`) on one lock file in the package's `LocalState`, which the
system never roams; its folder is made before it is opened. An instance that finds it held waits
for it with no deadline, and the system releases the lock of a holder that crashed. It then
judges the root again and takes the one the holder put in place, so no session is left writing
to an old store a copy has replaced. The wait blocks the thread that finds the store: one find
runs at a time and every storage call that arrives meanwhile waits for its answer, and the
storage host runs the store's calls on the IO runtime's blocking threads (`spawn_blocking`), so
the wait never holds the IO workers HTTP, image loads and background work share, nor the owner
thread. What a stalled holder still holds back is that instance's storage answers, the limit
[gaps](../gaps.md#sandbox-aware-file-access-is-partial) names. Staging folders an
earlier copy left are removed first, under that lock, by their exact name, folders only and no
link followed, at most `MAX_SWEPT` at a time. A copy or rename that fails removes its staging
folder and reports its own error, and a root another instance put in place first is kept. The
old store stays the store only where the new root was shown to hold none: after a copy that
failed, which the next start tries again, and for a store past the bound or holding a link,
which is never copied. The session reads and writes it there, so nothing is written to a new
root the next start would take for the store. A new root that could not be read stays the
store, so a session never goes back to an old store an earlier copy replaced. The Linux user
folders answer none
for a folder that is not there for the process, which is how a Flatpak shows a folder its
manifest does not expose.

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

- The compiler: every `GrantBackend` answers a sandbox and returns a `ResolveHold`; only the
  crate makes an `AccessScope`, which is `Send`; a `Grant` has no empty or oversized payload
  (`Grant::try_new`).
- Tests: `a_reacquired_path_is_handed_out_only_where_the_process_can_open_it`,
  `a_picks_access_survives_a_restart_through_the_bytes_the_application_kept` and
  `a_grant_the_system_no_longer_honours_answers_why` in `flui-system`'s `system_it`;
  `a_foreign_grant_is_refused_without_asking_the_system` and
  `a_path_the_seam_refuses_is_refused_before_the_file_system`, rows of
  `every_provider_keeps_the_system_services_contract`;
  `each_sandbox_reaches_its_own_folders_and_what_its_system_exposes` (`flui-system`) and
  `a_flatpak_is_a_process_with_its_description_whatever_its_environment_says`,
  `the_description_is_read_on_linux_alone_and_within_its_bound`,
  `a_description_out_of_reach_is_no_answer_and_still_a_sign_of_flatpak`,
  `a_key_file_list_is_split_where_its_writer_did_not_escape`,
  `a_manifests_filesystems_name_what_the_flatpak_reaches`,
  `an_app_sandbox_keeps_to_its_container_whatever_its_environment_names`,
  `a_description_that_is_no_file_is_never_opened` (Unix),
  `the_user_database_is_asked_only_when_a_sandbox_needs_it_and_never_home` (`flui-platform`);
  `a_sandbox_read_that_answered_nothing_is_asked_again`,
  `a_sandbox_read_that_panicked_is_asked_again`,
  `a_resolve_waiting_for_a_read_that_panicked_is_refused_unavailable`,
  `a_description_that_could_not_be_read_answers_no_sandbox`,
  `the_user_database_record_is_read_whole_from_a_buffer_too_small` (macOS and Linux),
  `a_dropped_resolve_leaves_the_queue_and_the_queue_is_bounded`,
  `a_scripted_grant_reads_back_as_the_doubles_and_no_system_takes_it` and the row
  `a_scripted_grant_is_foreign_to_every_system` (`flui-system`);
  `a_dropped_resolve_cancels_its_calls_and_stops_its_read` (Android, on every host);
  `a_resolve_let_go_of_starts_no_scope` (Apple, on every host, through the wiring
  `AppleGrants::resolve` calls);
  `a_packaged_applications_store_is_copied_over_once_and_nothing_is_overwritten`,
  `a_copy_that_fails_midway_leaves_no_root_and_the_next_start_copies_again`,
  `a_root_another_instance_put_in_place_first_is_kept`,
  `a_rename_that_fails_removes_its_staging_and_reports_its_own_error`,
  `staging_folders_an_earlier_copy_left_are_swept_by_exact_name_within_a_bound`,
  `a_sweep_follows_no_link` (Unix),
  `a_second_instance_waits_for_the_lock_and_takes_the_root_put_in_place`,
  `the_lock_is_taken_where_local_state_is_not_made_yet`,
  `calls_that_arrive_while_the_store_is_found_wait_for_one_find_and_one_migration`,
  `the_calls_that_waited_for_a_failed_find_share_its_refusal`,
  `a_store_call_that_waits_holds_no_io_worker` and
  `a_write_the_shutdown_lets_finish_answers_what_landed` (`flui-app`'s `host_storage_matrix`),
  `a_new_root_that_cannot_be_read_stays_the_store`,
  `a_staging_folder_already_there_is_an_error_not_a_shared_folder`,
  `a_store_that_was_not_copied_stays_the_store_and_no_new_root_is_made`,
  `a_description_that_could_not_be_read_is_no_container`,
  `a_container_that_could_not_be_read_is_read_again_at_the_next_call` and
  `the_user_database_record_is_read_whole_from_a_buffer_too_small` (Unix) (`flui-native`);
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
