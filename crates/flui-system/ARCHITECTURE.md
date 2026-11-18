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

## Picks and saves without a path

- A pick is a `PickedFile`, not a path, because not every system answers one; `PickedFile::form`
  says which it is (`PickedForm`). A desktop dialog answers where the file is (`Path`); a browser
  answers a file a page may read but never a path, which the pick carries as its name and
  contents (`Contents`), read when the dialog answers; a save handed to the browser as a download
  is `Download`, whose place and fate the browser does not report (it may be cancelled, blocked or
  renamed), so it carries the name it was offered under and no contents. Reading is bounded by the
  request (`OpenRequest::max_bytes`, by default `TransferLimits`' default, a drop's bound): the
  picks' declared sizes are checked before a byte is read and the bytes read are counted again,
  and more is `DialogError::TooLarge`. Each file is copied once, from the browser's buffer into the
  pick's own.
- A host whose dialog only suggests the filters checks every pick against them
  (`FileFilter::matches`, ADR-0360 §4) and refuses the whole answer with
  `DialogError::NotInFilter` when one matches none: the answer is what the user chose, and it is
  not what the request asked for.
- A save may carry the contents it writes (`SaveRequest::contents`), the one way to save where
  the system gives a page no file to write later. The handle writes them to a path the user
  chose on a thread of its own, into a new file beside it that is synced and renamed over it, so a
  failed write leaves the chosen file as it was, and answers once they are written (a failed write
  answers the system's code); a backend that hands over no path writes them itself and answers the
  file it wrote. Once the user has chosen, the write completes even if the caller let the dialog
  go, on every host. A save without contents answers where to save, and on the web, which has no
  path to answer, is refused with `NotOnThisPlatform`.

## The web providers (`web`)

- Each provider reaches its page through the acquiring widget's window, from the canvas its raw
  handle names (`web::page::Page`), never through the browser's globals; a widget in no window,
  or in one that is not a canvas, is refused with `NoWindow`. `ProviderContext::app_id` and
  `WindowRef::portal_parent` mean nothing to a page and are not read.
- What the providers decide without the DOM (the filters' mapping and checks, a save's name, a
  launch's route, what an exception means) is `web::rules`, tested on every host; the DOM side is
  `web::tests`, the lib test on wasm32 in headless Chrome (CI's `wasm-check` job), with user
  activation stated by the test and `window.open`, the save picker and the Badging API replaced,
  so no picker or window opens.
- **User activation, one answer.** A browser shows a picker or a new window only while the page
  handles a user's gesture. A call outside one is refused with `Unavailable` before the browser is
  asked (the browser would refuse it, or a file input and a popup blocker would drop it
  silently), as `flui-native`'s web vibration is, and the browser's own refusal of such a call
  (`NotAllowedError`, `SecurityError`, a blocked window) is answered with the same `Unavailable`;
  a browser without `navigator.userActivation` is asked and refuses itself.
- **Open.** The File System Access picker where the browser has it (Chromium, in a secure
  context) and can express the filters (each extension of ASCII letters, digits, `+` and `.`, at
  most 16 characters with its dot, Chromium's `VerifyIsValidExtension`); `<input type=file>`
  otherwise, its `accept` the filters' union. A picker the document may not show (`SecurityError`:
  a cross-origin frame, which may show an input) hands over to the input while the page still
  handles the gesture, and answers `Unavailable` once the gesture is spent. A dismissed picker
  answers `AbortError`. An input ends on its `change`, its `cancel` (Chromium 113, Firefox 91,
  Safari 16.4), `pagehide`, or, where an engine has no `cancel`, the window regaining focus with
  no `change` within `web::dialogs::FOCUS_GRACE_MS`, which answers a cancel: a pick reported later than
  that (a large file a phone copies first) is lost, the price of an end on every engine. A folder
  dialog answers `NotYetOnThisHost`.
- **Save.** The save picker writes the request's contents through a writable stream, aborted on
  a failure, after the user chose even if the dialog was dropped: the browser has made or emptied
  the chosen file by then. Without a picker, outside a gesture (which the picker needs and a
  download does not), or when the browser refuses the picker for want of either, the contents are
  handed to the browser as a download under the suggested name, given the first filter's
  extension when it has none, from an object URL revoked a minute later, and answered as
  `PickedForm::Download`. The answer reports no filter: the browser says none was selected.
- **One picker per page.** A page cannot close a browser's picker, and the browser shows one at
  a time; the runtime's `PagePicker` refuses a second with `Busy` while one is up, whichever
  window asked, and after its dialog was dropped: a picker of the API until its promise settles,
  an input until it ends as above, keeping itself alive until then.
- **Launching.** `http` and `https` open in a new browsing context: a blank one first, so a
  blocked window is seen (`window.open` answers `null`, answered `Unavailable`), its `opener` cut,
  then the page followed from a link of that blank document with `rel=noreferrer`, so the page
  cannot reach the application and is not told where it came from. Any other allowed scheme is
  handed to the system's handler by a link the application's page follows, which the HTML
  standard does without unloading the page; `ftp`, `ws` and `wss` are nothing a browser opens for
  the user (`NoHandler`). A page has no paths to open, reveal or trash.
- **The launcher entry** is the Badging API's count (`setAppBadge`, `clearAppBadge`), honouring
  the count alone and never confirmed: a browser badges only an installed application and does
  not say whether it did. A browser without the API answers `NotOnThisDevice`.
- **The message box** is a modal `<dialog>` beside the canvas, its buttons labelled in English. A
  page's own `alert` and `confirm` are not used: they block the page's one thread while they are
  up (no frames, owner turns or network callbacks), the page cannot close them when the caller
  drops the dialog or its window closes (ADR-0344), and they offer no Yes, No and Cancel. Escape
  closes the box with what closing means for its buttons; a Yes or No box keeps Escape from
  closing it and is shown again if the browser closes it anyway. `showModal` makes the rest of
  the document inert, so the box is modal to the page, every canvas window of it, where the
  contract says modal to its window, and two windows' boxes stack. The box is named by its title
  (its message when it has none) and described by its message: through ARIA element reflection
  (`ariaLabelledByElements`, `ariaDescribedByElements`) where the browser has it, otherwise by
  `aria-labelledby` and `aria-describedby` over ids the document counts, since two UI runtimes
  of one page mint the same window ids. It is styled through each element's style object, which
  a content security policy without `'unsafe-inline'` styles leaves in force.
- **Sharing** is `navigator.share` (a title, text and a link) while the page handles a gesture. A
  browser without the API is refused at acquisition with `NotOnThisDevice`, as the badge is; a
  share of files, or of a link other than `http` or `https`, answers `NotOnThisPlatform` (a page
  has no files at paths, and FLUI shares only the links a share target can open) before the
  browser is asked. A
  refusal while the browser itself said the gesture was there is the frame lacking the
  `web-share` permissions policy, which only its embedder grants (`NotDeclared`); without that
  word it is `Unavailable`. A share while one is still showing has no busy answer in the contract
  and is the browser's `InvalidStateError` (`Failed`). A dismissed sheet is not reported.
- Autostart, global shortcuts, the user folders, the jump list and the system information answer
  `NotOnThisPlatform`.

## Tests

- `crates/flui-system/tests/` (`system_it`, feature `testing`): the handles through the public API
  on a headless host over `testing::ScriptedSystem`, and the conformance table
  `every_provider_keeps_the_system_services_contract`, every service run against the doubles and
  against `SystemPlugin` on the host it runs on (the Windows shell or the Linux portal), asking
  nothing a user answers. Against the platform's providers it proves acquisition, the seam's
  refusals and the launcher entry's shared state only (its windows are headless), and the system's
  name and home folder.
- `win32::*::tests`: each Windows provider at its OS seam, with the stand-ins and the test owner
  of the crate's kit (`testing::win32`, test builds on Windows only); `win32::modal::tests` and
  `win32::dialogs::tests` run the dialog thread and its dismissal window with a stand-in dialog.
- `tests/portal.rs` and `tests/portal_services.rs` (Linux): the portal providers against a
  stand-in portal on a private bus.
- `web::rules::tests` on every host, and `web::tests` in headless Chrome (CI's `wasm-check` job
  runs `cargo test -p flui-system --lib --target wasm32-unknown-unknown`).