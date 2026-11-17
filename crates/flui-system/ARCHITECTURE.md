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

Threads and apartments:

- **A modal dialog runs on a thread of its own** (`win32::modal`), inside a single-threaded
  apartment, owned by the acquiring widget's window. A common item dialog or a message box runs a
  modal loop inside the call that shows it; on the owner thread that loop would stand in for the
  main loop, whose pacing is the host's (a capability crate cannot reach it), and would run inside
  the widget's call. Owned across threads, the dialog still disables its owner and stays above it,
  while the owner thread's own loop goes on delivering frames and owner turns. Before it shows the
  dialog, the thread makes a message-only dismissal window: dropping the `ModalHold` posts it a
  message, which the dialog's modal loop dispatches on the dialog's thread, where the dialog's own
  close runs (`IFileDialog::Close`, `EndDialog` for the box). A hold dropped before the dialog shows
  keeps it from showing; an answer reaches the caller only when nobody dismissed the dialog.
- **The shell's other work runs a request per thread** (`ShellThreads`): the launcher, launch at
  login, the jump list and the folders, each inside its apartment, at most
  `ShellThreads::MAX_IN_FLIGHT` per runtime; a request past that answers `Unavailable`, and a thread
  that unwinds gives back its place and drops its resolver. No file system call runs on the owner
  thread (ADR-0360 §2).
- **The share sheet, the hot keys and the taskbar run on the owner thread**, inside the apartment
  the host entered there: they call the window's own thread's objects and read no file. A share
  with files checks them on a thread first, and the sheet resolves them as `StorageFile`s on a
  thread under the request's deferral.

Launching never builds a command line. A URI goes to `ShellExecuteExW` with the `open` verb and its
scheme as the class (`SEE_MASK_CLASSNAME`), so only that scheme's registered handler takes it, and
a one-letter scheme cannot read as a drive. A document is opened first (`CreateFileW`, held without
delete sharing while it is handed on), judged by its final name (`GetFinalPathNameByHandleW`)
against `runs_when_opened` and by `AssocIsDangerous`, and handed on by that name. Reveal is
`SHOpenFolderAndSelectItems`; trash is `IFileOperation` with `FOFX_RECYCLEONDELETE` and
`FOF_WANTNUKEWARNING`, so an item the recycle bin cannot take asks the user before it goes for good.

A file dialog's filter becomes `*.ext` patterns, which the dialog matches ignoring case and to the
end of the name, as the contract does (ADR-0360 §3); it filters by name only, so a MIME type is
refused with `NotOnThisPlatform` rather than dropped (§4). A save names a typed file without an
extension after the selected filter (`SetDefaultExtension` with the first filter's first) and
reports the selected filter from `GetFileTypeIndex`.

The taskbar subclasses each window it shows something on, to show its last progress and badge again
on `TaskbarButtonCreated` (a button made late, Explorer restarted), and removes the subclass when
the window closes. `SetWindowSubclass` and its siblings are found by ordinal (410 to 413), as the
SDK's import library links them: version 5.82 of `comctl32.dll`, which a process without the
version 6 manifest entry loads, exports them by ordinal only, and a link by name keeps such a
process from starting. The badge is an overlay icon drawn here, a disk with the count.

Launch at login names its `Run` value after the executable's stem and a hash of its lower-cased
path, so two applications with one file name never overwrite each other's entry; the user's choice
under `StartupApproved\Run` is reported (`DisabledByUser`) and never written. A packaged process,
whose manifest owns its start-up task, is refused.

## Linux

The `portal` module reaches `org.freedesktop.portal.Desktop` on zbus's own runtime, one thread per
UI runtime (`Portal`): the launcher (`OpenURI`, `OpenDirectory`, `Trash`) and the file dialogs
(`FileChooser`). The services it has no portal provider for yet answer `NotYetOnThisHost`.

## Tests

- `crates/flui-system/tests/` (`system_it`, feature `testing`): the handles through the public API
  on a headless host over `testing::ScriptedSystem`, and the conformance table
  `every_provider_keeps_the_system_services_contract`, run against the doubles and against
  `SystemPlugin` on the host it runs on, asking nothing a user answers.
- `win32::*::tests`: each Windows provider at its OS seam; `win32::modal::tests` and
  `win32::dialogs::tests` run the dialog thread and its dismissal window with a stand-in dialog.
- `tests/portal.rs` (Linux): the portal providers against a stand-in portal on a private bus.
