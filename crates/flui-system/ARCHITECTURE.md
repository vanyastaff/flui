# flui-system architecture

`flui-system` is the first capability crate (ADR-0154 §11): the OS shell services the core host's
closed list leaves out. It depends on the contract crate (`flui-platform`) and on external crates
only, keeps no process-global state, and registers through `SystemPlugin`. What each platform
provides is in [`docs/gaps.md`](../../docs/gaps.md) and the System services rows of
[`platform-parity.toml`](../flui-native/platform-parity.toml); this file states the rules the code
keeps.

## Handles and backends

Each service is a capability marker, an owner-thread handle and a backend trait:

| Service | Handle | Backend | Answer |
|---------|--------|---------|--------|
| Launch (contract crate) | `Launcher` | `LauncherBackend` | `Completion` |
| File dialogs | `FileDialogs` | `FileDialogBackend` | `Dialog` |
| Message box | `MessageBoxes` | `MessageBoxBackend` | `Dialog` |
| Share | `Share` | `ShareBackend` | `Completion` |
| Launch at login | `LaunchAtLogin` | `LaunchAtLoginBackend` | `Completion` |
| Global shortcuts | `GlobalShortcuts` | `GlobalShortcutBackend` | `Completion` of a `ShortcutRegistration` |
| Taskbar | `Taskbar` | `TaskbarBackend` | synchronous `Result` |
| Jump list | `JumpList` | `JumpListBackend` | `Completion` |
| The OS | `Os` | `OsBackend` | `Completion` for a folder, a value for `info` |

The rules every backend relies on live in the handle, so the scripted doubles and every platform
have them: a malformed request is refused before the backend is asked (`MalformedRequest`,
`MalformedShare`, `MalformedTask`, `InvalidShortcut`), each bounded before anything is copied; a
window-bound handle (dialogs, message box, share, taskbar) refuses a widget in no window with
`NoWindow` and answers `WindowClosed` once its window closed, without asking; an answer the request
cannot have (a button the box does not offer, several picks for one, a filter index past the
request's, a relative folder) is `SystemFailure::MalformedAnswer`. Every answer settles at an owner
turn, never inside the call.

A window shows one dialog at a time, whichever handle asked: `FileDialogs` and `MessageBoxes`
share the runtime's `OpenDialogs` table, and a `Dialog` lasts while its caller holds it, ending the
system's dialog through the backend's `DialogHold` when it is dropped, its window closes or the
runtime retires (ADR-0344). A global shortcut stays registered while its `ShortcutRegistration` is
held; `ShortcutPresses::press` runs the listener at the next owner turn, and a press delivered after
the registration dropped runs nothing.

## Windows

The `win32` module provides every service. Each OS call sits behind a trait of its file
(`DialogSystem`, `ShellSystem`, `MessageBoxSystem`, `LoginRegistry`, `HotkeySystem`,
`TaskbarSystem`, `JumpListSystem`), whose one real implementation calls Win32 or COM; the provider's
rules (what a request becomes, what an answer means, who hears it on every exit) are tested beside
each file with the OS's answers as inputs, so no test shows a dialog, launches a handler, registers
a hot key or writes the user's login items. The owed desktop runs are in the parity rows.

The share sheet's seam (`ShareSystem`) and the folders' lookup take the system's place the same
way.

Every OS entry of the crate (each window procedure, enumeration callback and WinRT delegate) is one
call of `owner::panic_boundary::os_entry`, the crate's boundary of the same contract as
`flui-native`'s (ADR-0151 §16): a panic is logged and contained, its payload destroyed under
containment, and the OS gets the entry's answer after a panic. `cargo xtask panic-boundary` reads
this crate as well as `flui-native`.

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
- **The shell's other work runs a request per thread** (`ShellThreads`): the launcher, launch at
  login, the jump list, the folders, the share sheet's file check and its file resolution, each
  inside the apartment its calls ask for, at most `threads::MAX_IN_FLIGHT` per runtime in one count
  (`ShellPool`); a request past that answers `Unavailable`, and a thread that unwinds gives back its
  place and drops its resolver. A request still running `ShellThreads::ABANDON_AFTER` after it
  started is abandoned when the owner next needs a place: its place is reclaimed, its caller
  answered with the request's refusal, the abandonment logged, and its late answer discarded (the
  caller's resolver was taken, the request's ticket). No file system call runs on the owner thread
  (ADR-0360 §2).
- **The share sheet, the hot keys and the taskbar run on the owner thread**, inside the apartment
  the host entered there: they call the window's own thread's objects and read no file. The share
  sheet's files are resolved as `StorageFile`s in the multithreaded apartment, on a thread that only
  waits on WinRT's asynchronous calls.

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

The taskbar subclasses each window it shows something on, to show its last progress and badge again
on `TaskbarButtonCreated` (a button made late, Explorer restarted), lets that one message through
the window's message filter (an elevated process hears Explorer, at medium integrity, only so), and
removes the subclass when the window closes. `SetWindowSubclass` and its siblings are found by
ordinal (410 to 413) in the system folder's `comctl32.dll` (`LOAD_LIBRARY_SEARCH_SYSTEM32`, never a
copy beside the executable), as the SDK's import library links them: version 5.82, which a process
without the version 6 manifest entry loads, exports them by ordinal only, and a link by name keeps
such a process from starting. The badge is an overlay icon drawn here, a disk with the count.

A global shortcut's hot key id is never handed out twice in a runtime's life, so a `WM_HOTKEY` still
queued for a dropped shortcut finds no registration rather than a later one; the 0xBFFF ids Windows
gives an application end in `ShortcutError::Exhausted`.

Launch at login names its `Run` value after the executable's stem and a hash of its lower-cased
path, so two applications with one file name never overwrite each other's entry; the entry is
compared with the quoted path unit for unit, and a value longer than a command line is refused
before anything is allocated for it. The user's choice under `StartupApproved\Run` is reported
(`DisabledByUser`) and never written. A packaged process, whose manifest owns its start-up task, is
refused.

## Linux

The `portal` module reaches `org.freedesktop.portal.Desktop` on zbus's own runtime, one thread per
UI runtime (`Portal`): the launcher (`OpenURI`, `OpenDirectory`, `Trash`) and the file dialogs
(`FileChooser`). The services it has no portal provider for yet answer `NotYetOnThisHost`.

## Tests

- `crates/flui-system/tests/` (`system_it`, feature `testing`): the handles through the public API
  on a headless host over `testing::ScriptedSystem`, and the conformance table
  `every_provider_keeps_the_system_services_contract`, run against the doubles and against
  `SystemPlugin` on the host it runs on, asking nothing a user answers. Against the platform's
  providers it proves acquisition and the seam's refusals only (its windows are headless), and the
  system's name and home folder.
- `win32::*::tests`: each Windows provider at its OS seam, with the stand-ins and the test owner
  of the crate's kit (`testing::win32`, test builds on Windows only); `win32::modal::tests` and
  `win32::dialogs::tests` run the dialog thread and its dismissal window with a stand-in dialog.
- `tests/portal.rs` (Linux): the portal providers against a stand-in portal on a private bus.
