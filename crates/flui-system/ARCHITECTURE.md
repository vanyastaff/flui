# flui-system architecture

`flui-system` is the first capability crate (ADR-0154 §11): the OS shell services the core host's
closed list leaves out. It depends on the contract crate (`flui-platform`) and on external crates
only, keeps no process-global state, and registers through `SystemPlugin`. Each service is a
marker, a handle and a backend trait in this crate, a provider per platform, and a scripted backend
under `testing`. What each platform provides is in [`docs/gaps.md`](../../docs/gaps.md) and the
System services rows of [`platform-parity.toml`](../flui-native/platform-parity.toml); this file
states the rules the code keeps.

## Handles and backends

| Service | Handle | Backend | Answer |
|---------|--------|---------|--------|
| Launch (contract crate) | `Launcher` | `LauncherBackend` | `Completion` |
| File dialogs | `FileDialogs` | `FileDialogBackend` | `Dialog` |
| Message box | `MessageBoxes` | `MessageBoxBackend` | `Dialog` |
| Share | `Share` | `ShareBackend` | `Completion` |
| Autostart | `Autostart` | `AutostartBackend` | `Completion` |
| Global shortcuts | `GlobalShortcuts` | `ShortcutBackend` | a `ShortcutBinding`'s events |
| Launcher entry | `TaskbarEntry` | `TaskbarBackend` | none: the whole state is sent |
| User folders | `UserDirs` | `UserDirBackend` | `Completion` |
| Jump list | `JumpList` | `JumpListBackend` | `Completion` |
| The OS | `Os` | `OsBackend` | a value |

The rules every backend relies on live in the handle, so the scripted doubles and every platform
have them:

- Every handle is owner-thread and every answer settles at an owner turn, never inside the call
  that asked.
- A malformed request is refused before any backend is asked, with a typed reason
  (`MalformedRequest`, `MalformedShare`, `MalformedTask`, `MalformedAutostart`,
  `MalformedShortcut`), each bounded before anything is copied.
- A window-bound handle (the dialogs, the message box, the share sheet) refuses a widget in no
  window with `NoWindow` and answers `WindowClosed` once its window closed, without asking. An
  answer the request cannot have (a button the box does not offer, several picks for one, a
  filter index past the request's) is `SystemFailure::MalformedAnswer`.
- A request its caller can call off ends when its value drops: a `Dialog` (ADR-0344) and a
  `ShortcutBinding`, each through the hold its backend returned. A window shows one dialog at a
  time, whichever handle asked: `FileDialogs` and `MessageBoxes` share the runtime's
  `OpenDialogs` table, and a `Dialog` ends the system's dialog through the backend's
  `DialogHold` when it is dropped, its window closes or the runtime retires.
- `GlobalShortcuts` delivers a stream through `ShortcutFeed`, the one cross-thread queue here: at
  most `ShortcutFeed::MAX_QUEUED` presses and releases (past it the oldest goes, counted by
  `ShortcutBinding::dropped`), one whole binding (a newer `Bound` or `Changed` takes the place of
  the one queued) and the end, which is answered again after it.
- `TaskbarEntry` is the application's: the handles of one UI runtime share one state, a change
  that changes nothing is not sent, and the backend is always handed the whole state.
- The message box, the share sheet and the jump list came with their Windows provider. No portal
  has a message box or a share sheet, and the jump list is Windows', so the Linux provider refuses
  the three with `NotOnThisPlatform`.

## Windows (`win32`)

The `win32` module provides every service. Each OS call sits behind a trait of its file
(`DialogSystem`, `ShellSystem`, `MessageBoxSystem`, `ShareSystem`, `LoginRegistry`,
`HotkeySystem`, `TaskbarSystem`, `JumpListSystem`, the known folders' lookup), whose one real
implementation calls Win32 or COM; the provider's rules (what a request becomes, what an answer
means, who hears it on every exit) are tested beside each file with the OS's answers as inputs, so
no test shows a dialog, launches a handler, registers a hot key, writes a start-up entry or touches
the taskbar. The owed desktop runs are in the parity rows.

Every OS entry of the crate (each window procedure, subclass, enumeration callback and WinRT
delegate) is one call of `owner::panic_boundary::os_entry`, the crate's boundary of the same
contract as `flui-native`'s (ADR-0151 §16): a panic is logged and contained, its payload destroyed
under containment, and the OS gets the entry's answer after a panic. `cargo xtask panic-boundary`
reads this crate as well as `flui-native`.

Threads and apartments:

- **A modal dialog runs on a thread of its own** (`win32::modal`), inside a single-threaded
  apartment, owned by the acquiring widget's window. A common item dialog or a message box runs a
  modal loop inside the call that shows it; on the owner thread that loop would stand in for the
  main loop, whose pacing is the host's (a capability crate cannot reach it), and would run inside
  the widget's call. Owned across threads, the dialog still disables its owner and stays above it,
  while the owner thread's own loop goes on delivering frames and owner turns. Before it shows the
  dialog, the thread makes a message-only dismissal window; a thread that cannot make one shows
  nothing and answers `Unavailable`, since its dialog could not be dismissed. Dropping the
  `ModalHold` posts the window a message, which the dialog's modal loop dispatches on the dialog's
  thread, where the dialog's own close runs (`IFileDialog::Close`, `EndDialog` for the box). A hold
  dropped before the dialog shows keeps it from showing; an answer reaches the caller only when
  nobody dismissed the dialog. The thread's stage leaves `Showing` on every exit, a panic in the
  dialog's code included, before its dismissal window is destroyed, so no hold posts to a window
  that is gone.
- **A window holds one dialog thread at a time** (`ModalWindows`): the handle frees a window as soon
  as its dialog is dismissed, while the dialog closes on its own thread a little later and only then
  re-enables its owner, so a dialog asked for before that thread has left its dialog answers
  `DialogError::Busy` rather than come up under a window the old one re-enables.
- **The shell's other work runs a request per thread** (`ShellThreads`): the launcher, autostart,
  the jump list, the user's folders, the share sheet's file check and its file resolution, each
  inside the apartment its calls ask for, at most `threads::MAX_IN_FLIGHT` per runtime in one count
  (`ShellPool`); a request past that answers `Unavailable`, and a thread that unwinds gives back its
  place and drops its resolver. A request still running `ShellThreads::ABANDON_AFTER` after it
  started is abandoned when the owner next needs a place: its place is reclaimed, its caller
  answered with the request's refusal, the abandonment logged, and its late answer discarded (the
  caller's resolver was taken, the request's ticket). No file system call runs on the owner thread
  (ADR-0360 §2).
- **The share sheet, the hot keys, the taskbar and the system information run on the owner
  thread**, inside the apartment the host entered there: they call the window's own thread's
  objects and read no file. The share sheet's files are resolved as `StorageFile`s in the
  multithreaded apartment, on a thread that only waits on WinRT's asynchronous calls.

Launching never builds a command line. A URI goes to `ShellExecuteExW` as it is, with the `open`
verb, so Windows resolves the handler the user chose for its scheme (`UserChoice`, the default
browser); the seam admitted only the schemes its caller named, and a one-letter scheme is refused,
since the shell reads `c:…` as a path on drive C. A document is opened first (`CreateFileW` reading
its data, so the sharing rules apply, and sharing reading and writing but not deletion: while the
handle is held, until `ShellExecuteExW` returns, nobody can rename, move or delete the file; writing
stays shared, since refusing it would refuse a document another application keeps open). Its final
name (`GetFinalPathNameByHandleW`) must be on a local drive (a link that led to a share is refused
`NotLocal`), and pass `runs_when_opened` and `AssocIsDangerous`; it is handed on by that name.
Reveal is `SHOpenFolderAndSelectItems`; trash is `IFileOperation` with `FOFX_RECYCLEONDELETE` and
`FOF_WANTNUKEWARNING`, so an item the recycle bin cannot take asks the user before it goes for good.

A file dialog's filter becomes `*.ext` patterns, which the dialog matches ignoring case and to the
end of the name, as the contract does (ADR-0360 §3); it filters by name only, so a MIME type is
refused with `NotOnThisPlatform` rather than dropped (§4). A save names a typed file without an
extension after the selected filter (`SetDefaultExtension` with the first filter's first) and
reports the selected filter from `GetFileTypeIndex`.

The launcher entry is shown on the taskbar button of each window a `TaskbarEntry` was acquired in,
one state for all of them, since every top-level window has a button of its own. The provider
subclasses each such window to show the entry again on `TaskbarButtonCreated` (a button made late,
Explorer restarted), lets that one message through the window's message filter (an elevated process
hears Explorer, at medium integrity, only so), and removes the subclass when the window closes.
`SetWindowSubclass` and its siblings are found by ordinal (410 to 413) in the system folder's
`comctl32.dll` (`LOAD_LIBRARY_SEARCH_SYSTEM32`, never a copy beside the executable), as the SDK's
import library links them: version 5.82, which a process without the version 6 manifest entry
loads, exports them by ordinal only, and a link by name keeps such a process from starting. The
count is an overlay icon drawn here, a disk with the count, `99+` above 99. With no window shown on,
the taskbar is not reached.

A global shortcut is bound to the trigger it prefers, or not at all: Windows has no settings an
application binds through, so a shortcut with no preferred trigger, or whose chord another
application holds, is left out of `ShortcutEvent::Bound`. A trigger's key is its virtual key (a
letter's or digit's ASCII upper case, which names the key that types it on the active layout; a
named key's fixed one), registered with `MOD_NOREPEAT`; `WM_HOTKEY` reports no release, so a
binding hears presses only. A hot key id is never handed out twice in a runtime's life, so a
`WM_HOTKEY` still queued for a dropped binding finds no shortcut rather than a later one; a binding
that needs more of the 0xBFFF ids than are left ends with `ShortcutError::Exhausted`, and none is
handed out.

Autostart names its `Run` value after the executable's stem and a hash of its lower-cased path, so
two applications with one file name never overwrite each other's entry; the entry is compared with
the quoted path unit for unit, and a value longer than a command line is refused before anything is
allocated for it. The user's choice under `StartupApproved\Run` is never written: a request to
enable an entry the user turned off answers `Disabled`, what the system grants. A request's reason
is not shown, since the `Run` key asks nobody. A packaged process, whose manifest owns its start-up
task, is refused.

A user folder is `SHGetKnownFolderPath`'s, on a shell thread, since a redirected folder may reach
the network; `PublicShare` is `FOLDERID_Public`. A folder the user does not have answers none, and
a relative path the system's contract excludes is `MalformedAnswer`.

## macOS (`macos`)

The `macos` module provides every service but the jump list, which macOS has no form of (the
Dock menu is the application delegate's, the host's). Each provider's rules (what a request
becomes, what an answer means) are functions of its file over plain values, compiled and tested on
every host (`macos::*::tests`); the AppKit, Foundation, ServiceManagement and Carbon calls are the
file's `macos`-only `appkit` module, checked by CI's macOS type-check. The owed Mac runs are in the
parity rows.

Threads:

- **AppKit runs on the main thread**, the UI runtime's owner thread in a macOS application: the
  panels, the alert, the share picker, the Dock tile and the hot keys are acquired with a
  `MainThreadMarker`, and a runtime whose owner thread is not the main thread (the headless host)
  is refused them with `NotYetOnThisHost` before AppKit is touched.
- **Work that touches the file system or waits on a daemon runs on a global dispatch queue**
  (`macos::queue::WorkQueue`): the launcher, the share picker's file check, the login item and the
  user's folders, at most `WorkQueue::MAX_IN_FLIGHT` per runtime; a request past that answers its
  refusal with `Unavailable`, and a work item gives its place back on every exit. No file system
  call runs on the main thread (ADR-0360 §2).

Every block, work item, Objective-C method and Carbon handler the system calls is one call of
`owner::panic_boundary::os_entry`, and the one class the crate declares (the open panel's name
check) keeps its ivars in `ContainedIvars`, which `cargo xtask panic-boundary` checks here as in
`flui-native`.

A feature newer than the floor is behind a gate of `macos::version` (ADR-0244 §4): a marker with its
`SINCE`, a `Since` proof the call takes, and a compile error for a gate at or below the floor, which
`macos/floor.rs` states, generated from `platform-support.toml` by `cargo xtask platform-floor`. The
floor is 10.13 on x86_64 and 11.0 on arm64, so a gate between them exists on x86_64 only. A class
above the floor is looked up at run time, never linked: `UTType` (macOS 11, in a framework 10.15
lacks) through `AnyClass::get`, `SMAppService` (macOS 13) through objc2's lookup behind the proof.

A dialog and the alert are sheets on the acquiring widget's window (`beginSheetModalForWindow:`),
and a window with no AppKit window answers `NoWindow`. The sheet's completion handler and the
dialog's hold share the caller's resolver (`macos::sheet::Pending`): the first to come takes it, so
a hold dropped while the sheet is up (the caller let go, the window closed, the runtime retired)
ends the sheet (`endSheet:returnCode:`), and the handler that ending runs answers nobody
(ADR-0344). A filter becomes the panel's allowed content types: an extension the `UTType` of its last
part, a MIME type its type, a MIME family (`image/*`, `audio/*`, `video/*`, `text/*`, `font/*`) the
family's system type; another family is refused `NotOnThisPlatform` rather than widened. An open
panel with a several-part extension (`tar.gz`) checks names itself through its delegate
(`panel:shouldEnableURL:`), which enables a folder to browse or a file the contract's rule admits
(ADR-0360 §3, §4). A panel has no filter menu, so every filter's types are allowed at once and a save
reports no selected filter. The alert adds its buttons default first, so AppKit puts it rightmost and
gives Escape to the button titled Cancel.

Launching opens no shell. A URI goes to `NSWorkspace` `openURL:` as written once
`URLForApplicationToOpenURL:` finds a handler. A document is opened first (`open(2)`, following
links), and its final path (`F_GETPATH`) is judged: the name floor (`runs_when_opened`), an execute
bit on a regular file, and a folder Launch Services calls an application (`NSURLIsApplicationKey`)
are `NotADocument`; only then is the final path handed to `openURL:`, while the file is still open.
Reveal is `activateFileViewerSelectingURLs:` once the path exists; trash is `NSFileManager`
`trashItemAtURL:`, a volume with no trash `NotOnThisDevice`.

The share picker is a popover on the window's content view, shown at the owner turn where the file
check settles, unless the window closed meanwhile; it shares the text as a string, the link as a URL
and each file as a file URL, and has no title.

Autostart is the application's own login item, `SMAppService.mainAppService`. The user's switch-off
in System Settings (`requiresApproval`) is never registered over: enabling then answers `Disabled`.
An unbundled executable has no item (`NotDeclared`).

A global shortcut is a Carbon hot key (`RegisterEventHotKey`), which needs no Accessibility
permission and reports the release. The runtime's handler on the application event target is
installed with its first binding; a shortcut is bound to the trigger it prefers or left out of
`Bound` (no preferred trigger, F21 to F24, which have no key code, or a chord another application
holds); a letter is bound by its position on an ANSI keyboard; a hot key number is never handed out
twice in a runtime's life, and the handler passes on an event whose number it does not hold.

The launcher entry is the application's Dock tile: the count its badge, in full; a known extent a bar
over the application's icon in the tile's content view. The Dock draws the tile once per change, so
an indeterminate bar could not move and shows no progress.

A user folder is `URLsForDirectory:inDomains:` in the user's domain (Movies for videos, Public for
the public share; there is no templates folder), the home folder `NSHomeDirectory`; a folder the
user does not have answers none. The system information is `NSProcessInfo`'s version, read once per
runtime, and the machine's processor, Arm64 for a process Rosetta translates.

## Linux (`portal`)

- One `Portal` per UI runtime (`ProviderContext::runtime_local`): a thread of its own runs a
  `zruntime` executor; every request is a task there. Every file-system call (opening a file for
  the portal, reading `user-dirs.dirs`, reading `/proc/self/exe`) runs on `zruntime::unblock`'s
  pool, never on the owner thread or the portal's, so a stalled mount delays that request alone.
- A request reaches the portal through `Bus::desktop`: the connection, the portal's current
  unique owner (an installed portal that is not running is started first), and, for an
  unsandboxed application with an id, the application named to that owner through
  `org.freedesktop.host.portal.Registry`, once per connection and portal instance and before
  its first call. Every call goes to that owner, and only signals that owner sends are heard.
- Before a request depends on an interface it checks the interface's `version`
  (`Desktop::require`), read once per portal instance; an absent interface or an older version
  is `NotOnThisDevice`. A portal that is not on the bus is `Unavailable`.
- A request that shows something names the acquiring widget's window: `with_parent` asks
  `WindowRef::portal_parent` (ADR-0365) and the task holds the `PortalParent` until the portal
  answered the request (a shortcut binding keeps it across a retry until its `BindShortcuts`
  was answered). The window's close releases the export whoever holds it, and a request with no
  window, or a closed one, names none.
- Every object path is the one the portal will give (`request::object_path`), so each
  subscription is in place before the call that could answer it.
- When the UI runtime's portal ends, its bus is retired: every request still showing something
  closes its `Request`, every shortcut session closes its `Session`, and the thread stops when
  the tasks have ended or at `DRAIN_BOUND`. Every call is bounded by `METHOD_TIMEOUT`.
- A shortcut binding renews its session on the portal that replaces a departed one, and retries
  a failure that can clear (`Unavailable`, a bus failure) on `RETRY`, at most `RETRIES` times in
  a row, then ends with that failure. Nothing is polled.
- Autostart goes through the `Background` portal only; the command line is this executable, and
  one a desktop entry cannot hold unquoted is refused, since the portal writes it unquoted. FLUI
  writes no autostart entry of its own.
- The launcher entry is the Unity `LauncherEntry.Update` signal under
  `application://<id>.desktop`; an application with no id has none (`NotDeclared`). One task per
  UI runtime sends it, taking the newest state each time it is free, so a burst of changes ends
  on the last one.
- The system information has no Linux provider yet and answers `NotYetOnThisHost`.

## Tests

- `crates/flui-system/tests/` (`system_it`, feature `testing`): the handles through the public API
  on a headless host over `testing::ScriptedSystem`, and the conformance table
  `every_provider_keeps_the_system_services_contract`, every service run against the doubles and
  against `SystemPlugin` on the host it runs on (the Windows shell, AppKit or the Linux portal),
  asking nothing a user answers. Against the platform's providers it proves acquisition, the seam's
  refusals and the launcher entry's shared state only (its windows are headless), and the system's
  name and home folder.
- `win32::*::tests`: each Windows provider at its OS seam, with the test owner
  (`testing::owner`) and the stand-ins of the crate's kit (`testing::win32`, test builds on
  Windows only); `win32::modal::tests` and
  `win32::dialogs::tests` run the dialog thread and its dismissal window with a stand-in dialog.
- `tests/portal.rs` and `tests/portal_services.rs` (Linux): the portal providers against a
  stand-in portal on a private bus.
- `macos::*::tests` (every host): each macOS provider's rules as a model, with the system's answers
  as inputs and `testing::owner::TestOwner`'s turns; the AppKit calls compile only for macOS.
