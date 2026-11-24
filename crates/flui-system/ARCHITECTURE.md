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
| Grants | `Grants` | `GrantBackend` | `Completion` |
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
  filter index past the request's, a pick of a form the request does not ask for: a handed-over
  folder to a file dialog or a save, a handed-over file to a folder dialog, a download to an
  open) is `SystemFailure::MalformedAnswer`. What a backend hands over rather than names by a
  path is held to the open request's bounds by the handle as well: a folder of more files than
  `max_entries` is `TooManyEntries`, contents past `max_bytes` together are `TooLarge`.
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
- A provider that hands a pick over without a path (the browser, iOS, Android) reads it itself, by the
  one set of rules in `dialog::handed_over`: the picks are held to the filters by name and type,
  their declared sizes to the request's `max_bytes` before a byte is read, and the bytes read are
  counted again.
- A message box names its default button and may label its buttons and carry a detail
  (`MessageBoxRequest`). The handle refuses a default or a label for a button the box does not
  offer, a blank label, and two buttons labelled alike. A box without a title shows none on any
  host. A host that cannot label a button refuses a request that labels one before anything is
  shown (`NotYetOnThisHost`), rather than show other words than its author wrote. What each host
  shows is under its heading below, and in `MessageBoxRequest`'s documentation.

## Windows (`win32`)

Each OS call of the `win32` module sits behind a trait of its file
(`DialogSystem`, `ShellSystem`, `MessageBoxSystem`, `ShareSystem`, `LoginRegistry`,
`HotkeySystem`, `TaskbarSystem`, `JumpListSystem`, the known folders' lookup), whose one real
implementation calls Win32 or COM; the provider's rules (what a request becomes, what an answer
means, who hears it on every exit) are tested beside each file with the OS's answers as inputs, so
no test shows a dialog, launches a handler, registers a hot key, writes a start-up entry or touches
the taskbar. The owed desktop runs are in the parity rows.

Every OS entry of the crate (each window procedure, subclass, enumeration callback and WinRT
delegate) is one call of `flui_foundation::panic::os_entry`, the boundary `flui-native`'s entries
go through too (ADR-0151 §16): a panic is logged and contained, its payload destroyed under
containment, and the OS gets the entry's answer after a panic, or the process ends when that
answer panics as well.

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

A file dialog's filter becomes the dialog's patterns, which it matches ignoring case and to the end
of the name, as the contract does (ADR-0360 §3, ADR-0374): `*.ext` for an extension, the name itself
for a file name (`Makefile`), `*.*` for a filter of every file. It filters by name only, so a MIME
type is refused with `NotOnThisPlatform` rather than dropped (ADR-0360 §4). A save names a typed
file without an extension after the selected filter (`SetDefaultExtension` with the first filter's
first, when it has one) and reports the selected filter from `GetFileTypeIndex`. A request that
asks for hidden files adds `FOS_FORCESHOWHIDDEN`.

A dialog and the box are owned by the widget's window, which is what makes them modal to it: a
window whose handle names no Win32 window answers `NoWindow`, and nothing is shown unowned. The
folder a dialog opens in is named to it as a shell item, and the shell's parser does not read a
verbatim path, so `win32::verbatim` gives `\\?\C:\…` and `\\?\UNC\server\share\…` their plain form
first, where that names the same file; the launcher hands the shell a final name the same way. A
folder that is not there (the shell answers file or path not found, an invalid drive, a drive not
ready, a share that does not answer) is left out and the dialog opens where the system would, as
every host does with a remembered folder that has gone since. A folder the shell cannot name for
another reason fails the request with the shell's code before anything is shown. The check is the
shell's own, on the dialog's thread: no file system call runs on the owner thread (ADR-0360 §2).

The message box is `MessageBoxW`, which has one text, a caption and a fixed set of buttons. The
detail goes below the message after an empty line; a request without a title gets an empty caption,
never the null one the system shows as "Error" whatever the level; the default button is
`MB_DEFBUTTON` of its place among the buttons. `MessageBoxW` cannot label a button, so a request
that labels one is refused with `NotYetOnThisHost` before anything is shown, rather than shown with
the system's words where its author wrote others (`MessageBoxRequest::is_labelled`).

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

macOS's nearest form of a jump list, the Dock menu, is the application delegate's, the host's, so
the `macos` module has no jump list. Each provider's rules (what a request
becomes, what an answer means) are functions of its file over plain values, compiled and tested on
every host (`macos::*::tests`, and `apple::*::tests` for what iOS shares); the AppKit, Foundation,
ServiceManagement and Carbon calls are the
file's `macos`-only `appkit` module, checked by CI's macOS type-check. The owed Mac runs are in the
parity rows.

Threads:

- **AppKit runs on the main thread**, the UI runtime's owner thread in a macOS application: the
  panels, the alert, the share picker, the Dock tile and the hot keys are acquired with a
  `MainThreadMarker`, and a runtime whose owner thread is not the main thread (the headless host)
  is refused them with `NotYetOnThisHost` before AppKit is touched.
- **Work that touches the file system or waits on a daemon runs on a global dispatch queue**
  (`apple::queue::WorkQueue`): the launcher, the share picker's file check, the login item and the
  user's folders, at most `WorkQueue::MAX_IN_FLIGHT` per runtime; a request past that answers its
  refusal with `Unavailable`, and a work item gives its place back on every exit. No file system
  call runs on the main thread (ADR-0360 §2).

Every block, work item, Objective-C method and Carbon handler the system calls is one call of
`flui_foundation::panic::os_entry`, and the one class the crate declares (the open panel's name
check) keeps its ivars in `ContainedIvars`, as in
`flui-native`.

A feature newer than the floor is behind a gate of `apple::version` (ADR-0244 §4): a marker with its
`SINCE`, a `Since` proof the call takes, and a compile error for a gate at or below the floor, which
`apple/floor.rs` states, generated from `platform-support.toml` by the `platform_floor` test of `flui-cli`. The
floor is 10.13 on x86_64 and 11.0 on arm64, so a gate between them exists on x86_64 only. macOS and
iOS each have a version type and a floor of their own, so a gate written for one is never asked of
the other. A class
above the floor is looked up at run time, never linked: `UTType` (macOS 11, in a framework 10.15
lacks) through `AnyClass::get`, `SMAppService` (macOS 13) through objc2's lookup behind the proof.

A dialog and the alert are sheets on the acquiring widget's window (`beginSheetModalForWindow:`),
and a window with no AppKit window answers `NoWindow`. The sheet's completion handler and the
dialog's hold share the caller's resolver (`apple::modal::Pending`): the first to come takes it, so
a hold dropped while the sheet is up (the caller let go, the window closed, the runtime retired)
ends the sheet (`endSheet:returnCode:`), and the handler that ending runs answers nobody
(ADR-0344). A filter is what the panel allows: from macOS 11 its content types (an extension the
`UTType` of its last part, a MIME type its type, a MIME family, `image/*`, `audio/*`, `video/*`,
`text/*` or `font/*`, the family's system type; another family is refused `NotOnThisPlatform`
rather than widened); below macOS 11, on an Intel Mac, its file types, the extensions' last parts,
and a filter with a MIME type answers `NotOnThisDevice`. macOS admits what conforms to an allowed
type (`.swift` under `txt`) and derives a type from a name's last part, so an open panel with a
filter holds every item to the contract's rule through its delegate (`panel:shouldEnableURL:`): a
folder to browse, or a file whose name ends in an extension, which reads nothing of the file, or
whose type conforms to a MIME pattern's (ADR-0360 §3, §4). The item judged is the one the panel
answers, an alias's or a link's target, and that target is what the pick hands out, so the
delegate and the handle's name check judge one file. Under a MIME pattern, where the handle does
not check, a pick whose name and type both fail the rule answers the handle's `NotInFilter`; a
pick whose type cannot be read is not refused for that. A save's pick names a file to create and
is not checked. A panel has no filter menu, so every filter is allowed at once and a save reports
no selected filter. For the same reason a filter of every file leaves the panel with no allowed
types, as a request without filters does. A filter that names a file exactly has no content type,
so the panel is given no allowed types then either (ADR-0374 §4): the open panel's delegate enables
what the request's filters name or type, and a save panel takes any name and appends no extension.
A request that asks for hidden files sets `showsHiddenFiles`. `openPanel` and `savePanel` answer nil
where the process may not show a panel (an App Sandbox without the user-selected-file entitlement)
or the panel service fails; the binding declares them never nil and would panic, so the provider
sends them itself and answers `Unavailable` for nil. It makes a panel through its panel source
before it looks for the window, so a test drives `open` and `save` with a source that makes none.
The mapping of filters to content types is `apple::content_types`, which iOS's picker shares.

An alert has two texts and no title bar: the title is its message text over the message and the
detail, and with no title the message is the message text over the detail. Its buttons are added in
the order `MessageButtons::answers` gives, which AppKit lays out from the right, each titled by the
request's label or by its English word. The provider sets every button's key itself, so the rule
holds under any label: Return for the request's default button, Escape for the one that answers
Cancel unless it is the default (a button has one key), none for the rest.

A path reaches Foundation as the file system's bytes (`fileURLWithFileSystemRepresentation:`) and
comes back the same way (`getFileSystemRepresentation:`), so a name that is not UTF-8, which a FAT,
exFAT, SMB or NFS volume may hold, names its file.

Launching opens no shell. A URI goes to `NSWorkspace` `openURL:` as written once
`URLForApplicationToOpenURL:` finds a handler. Only a regular file or a folder is a document, and
nothing else is ever opened: the path is examined first (`stat`), since opening is itself an act
on a pipe or a device (a pipe's writer is let through, a terminal may become the process's), then
opened following links, without blocking and without taking a terminal
(`O_NONBLOCK | O_NOCTTY | O_CLOEXEC`: a node swapped in between must neither hold the work item's
place nor become the controlling terminal), and what the descriptor holds is examined again
(`fstat`). Its final path
(`F_GETPATH`) is judged: the name floor (`runs_when_opened`), an execute bit on a regular file, and
a folder Launch Services calls an application (`NSURLIsApplicationKey`) are `NotADocument`; only
then is the final path handed to `openURL:`, while the file is still open. Reveal is
`activateFileViewerSelectingURLs:` once the path exists; trash is `NSFileManager`
`trashItemAtURL:`, a volume with no trash `NotOnThisDevice`. A volume that stops answering holds a
request's place until it answers.

The share picker is a popover on the window's content view, shown at the owner turn where the file
check settles, unless the window closed meanwhile; it shares the text as a string, the link as a URL
and each file as a file URL, and has no title.

Autostart is the application's own login item, `SMAppService.mainAppService`. The user's switch-off
in System Settings (`requiresApproval`) is never registered over: enabling then answers `Disabled`.
An unbundled executable has no item (`NotDeclared`).

A global shortcut is a Carbon hot key (`RegisterEventHotKey`), which needs no Accessibility
permission and reports the release. Each runtime installs its handler on the application event
target with its first binding. A shortcut is bound to the trigger it prefers or left out of `Bound`:
no preferred trigger, F21 to F24 (no key code), or a chord another application holds, which an
exclusive registration (`kEventHotKeyExclusive`) refuses. A letter or digit is the key of the main
block (`kVK_ANSI_A` to `kVK_ANSI_Grave`, never the keypad, which types the digits on every layout)
that types it on the current layout while the chord's Shift and Command are held (`UCKeyTranslate`
over the current input source), so `⌘S` on Dvorak – QWERTY ⌘ is the key QWERTY has it on; Control
and Option are left out of the question, since under them a key types another character than the
one its chord is read by. Where no such key types it (a non-Latin layout, a digit AZERTY types only
with Shift) it is its ANSI position. The runtime observes
`kTISNotifySelectedKeyboardInputSourceChanged` while its handler is installed and, when the layout
changes, moves every shortcut whose key is another one now: all that move are unregistered before
any is registered, since two may trade keys, and a binding whose shortcuts changed by it (one whose
new chord is held leaves, one that was left out and whose new chord is free joins) hears `Changed`
with the ones bound. Two runtimes of one process each move their own, so a pair of chords that
trade keys across them can collide. Carbon hands every hot key to every handler on the application
target, so a hot key's number is unique in the process (one counter, ADR-0097 §2, folded into the
`EventHotKeyID`'s signature and id) and never handed out again: a handler passes on an event whose
number it does not hold, for the next runtime's handler, and a process that handed out every number
ends a binding that needs one with `ShortcutError::Exhausted`.

The launcher entry is the process's Dock tile: the count its badge, in full; a known extent a bar
over the application's icon in the tile's content view, made once per runtime and updated. The Dock
draws the tile once per change, so an indeterminate bar could not move and shows no progress. The
entry's state is a UI runtime's (`TaskbarEntry`), while the tile is the process's: in a process with
two runtimes, the last to set its entry is what the tile shows.

A user folder is `URLsForDirectory:inDomains:` in the user's domain (Movies for videos, Public for
the public share; there is no templates folder), the home folder `homeDirectoryForCurrentUser`; a
folder the user does not have answers none. The system information is `NSProcessInfo`'s version,
read once per runtime, and the machine's processor, Arm64 for a process Rosetta translates.

## iOS (`ios`)

Foundation's side of an `ios` provider is the `apple`
module, shared with macOS (the work queue, the version gate, the content types, a share's items,
the folders, the system information); each provider's rules are functions over plain values,
tested on every host (`ios::*::tests`, `apple::*::tests`), and the UIKit calls are the file's
`ios`-only `uikit` module, checked by CI's iOS type-check. The owed Simulator runs are in the
parity rows.

Threads, as on macOS: UIKit runs on the main thread, and a runtime whose owner thread is another
one is refused the launcher, the dialogs, the message box, the share sheet and the system
information with `NotYetOnThisHost`; judging and reading a picked document, writing the file a
save exports, checking a shared file and naming a folder run on `apple::queue::WorkQueue`. Every
block, delegate method and work item is one call of `os_entry`, and the picker's delegate keeps
its ivars in `ContainedIvars`.

A picker, an alert and a share sheet are presented from the requesting window's root view
controller, or from the controller it presents on top, as deep as that chain goes
(`ios::presenter`); a window with no UIKit window answers `NoWindow`. UIKit presents only from a
controller that presents nothing and whose presented controller is not leaving, and otherwise
presents nothing and calls nobody. A request that finds a controller of the chain being dismissed
(a dialog asked for in another's answer, whose controller is still animating away) waits for that
transition to end (`transitionCoordinator`) and looks at the chain again, at most
`MAX_DISMISSALS_AWAITED` times; a chain that does not settle, or a dismissal with no transition to
hear the end of, answers `Busy` (the share sheet `Unavailable`), and a request whose dialog ended
or whose window closed while it waited presents nothing. Every presentation is checked right
after it was asked for, so one UIKit did not take answers `Busy` at once. Whatever is presented
with a popover (an iPad's share sheet) has it anchored in the middle of the presenter's view, with
no arrow, since a request names no control to point at and an unanchored popover raises an
exception. The anchor, and the presentation's delegate, are set once UIKit took the controller:
asking a controller that is never presented for its presentation makes one that keeps it alive
for good, so after a refusal nothing of the presentation keeps what the request made. A
controller is dismissed through the controller that presents it: asked of a controller that
itself presents another (a share sheet over a picker), UIKit takes down only what is above it.
UIKit reports a controller it cannot make or present by raising an Objective-C exception, which
would end the process from inside a widget's call: each call that makes, presents or dismisses a
controller runs where the exception is caught (`ios::presenter::guarded`), which logs its name and
reason and answers `SystemFailure::MalformedAnswer`, and an initializer that answers no picker
answers the same.

UIKit tells nobody when other code dismisses a presented controller, so the controller owns what
answers it: an alert its actions' handlers, a picker its delegate, held as an associated object
since its delegate property is weak. They hold the caller's resolver, and the dialog's hold keeps
only weak references (`apple::modal::WeakPending`), so the hold dismisses a controller still up
(the caller let go, the window closed, the runtime retired) and takes the resolver first, and a
controller UIKit releases unanswered drops the resolver with its handlers: the caller's completion
ends `ProviderDropped` and the window is free, instead of busy for good. A picker answers once: a
pick, a cancel, or a swipe that dismisses it (its own delegate and its presentation's both report
an end, and the first takes the caller), which is a cancel. A handler takes what it holds of the
caller out of its block when it runs, so that is dropped inside the panic boundary and not when
UIKit disposes of the block.

An open is `UIDocumentPickerViewController` opening documents in place, filtered by content type
as on macOS from iOS 14 (`UTType`, looked up at run time; a type the application's bundle declares
is found by its extension, ADR-0361); below it an open without filters shows every document and
one with filters answers `NotOnThisDevice`. A filter of every file, or one that names a file
exactly, which no type expresses, shows every document (ADR-0374 §4). The picker shows no title,
no accept label and has no option for hidden files. Nothing
is read, and nothing downloaded, for an answer the request will not get: under the picks'
security-scoped access, which is started before a document is judged and stopped after it is
read, every pick is first held to the contract's rule, by its name and then its type's conformance
to a MIME pattern's (`NotInFilter`), and the sizes the system declares for the picks, which it
knows of a document still in the cloud, are summed against the request's `max_bytes`
(`TooLarge`). Only then is each read through `NSFileCoordinator`, which downloads a file a
provider keeps in the cloud, and bounded again by the size its opened file reports. A folder
dialog answers `NotYetOnThisHost`. A save exports: its contents are written to a file of the
request's name (the suggested name, or `Untitled`, completed by the first filter as a typed name is
on the portal, so a hidden name stays as it is) in a folder of its own in the temporary directory, the picker moves that file where the
user chose, and the folder is removed when the export ends, whichever way, a picker UIKit did not
take included; a save without contents answers `NotOnThisPlatform`.

The message box is a `UIAlertController` in the alert style, which no tap and no swipe dismisses:
the title its title, the message its message with the detail below it after an empty line, the
request's default button the preferred action (bold, and Return's), and the button closing the box
means (OK alone, or Cancel) the cancel-style action (Escape's); a Yes or No box has none. Each
action is titled by the request's label or its English word; the level is not shown. The
share sheet is `UIActivityViewController`, handed the same items as on macOS once the files were
checked, dismissed when its window closes while it is up; it shows no title. A link goes to
`UIApplication` `openURL:options:completionHandler:` as written, and a completion that reports no
application took it is `NoHandler`; `canOpenURL:` is not asked, since it answers only for schemes
the application lists in `LSApplicationQueriesSchemes`. A user folder is the application
container's (every iOS application is sandboxed): the home folder `NSHomeDirectory`, Documents the
container's, and the folders a container does not hold none. The system's name is the device's
(`UIDevice` `systemName`: iOS, or iPadOS on an iPad).

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
- A file dialog's filter is globs and MIME types (ADR-0360 §4, ADR-0374 §4): an extension in lower
  and in upper case, a file name as written and in both cases, `*` for a filter of every file. The
  folder a dialog opens in is a hint the portal may not use, and the file chooser has no option
  for hidden files, so a request that asks for them keeps the user's own toggle.
- A save's typed name with no extension is completed by the provider after the portal answered,
  with the first extension of the filter the portal reports as selected
  (`FileFilter::completes`): GTK's chooser, behind the portal on GNOME, answers the name as typed,
  where Windows' and macOS's dialogs complete it themselves. The portal asked the user about
  replacing the name as typed, so a completed name a file already has is not used and the name
  stands as typed. Nor is a name completed in the document portal's folder (`$XDG_RUNTIME_DIR/doc`
  or `/run/flatpak/doc`), where a sandboxed application reaches the file the portal granted, by
  the name it answered, and no other. A hidden name (`.env`) is not completed on any host: its
  leading dot starts its name, as Windows' dialog reads it. The rule is the provider's rather than
  the handle's because the check reads the file system, off the owner thread, and because the
  other hosts' dialogs complete the name before they ask about replacing a file, which a rule
  applied after the answer cannot.
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
- A user folder is the one `user-dirs.dirs` names, answered only when it is there for the process:
  a Flatpak sees the user's folders only as its manifest exposes them, which nothing but the file
  system shows, and the layer names no folder the process cannot open.

## Android (`android`)

Each `android` provider's rules are functions of its file over plain values, compiled and tested
on every host (`android::*::tests`), and the dialogs' whole path runs there over a stand-in
Activity and picker (`testing::android`); the JNI calls are the module's Android-only `java`
module, checked by CI's Android type-check. The owed device runs are in the parity rows.

**Java only as a leaf (ADR-0377).** Android's own classes (`Intent`, `ContentResolver`,
`AlertDialog.Builder`, `Environment`) are called from Rust. The crate ships two classes
(`android/java`, `dev.flui.system`), each forced by the platform:

- `FluiDocuments` is a content provider, a component the manifest must name and Android
  instantiates by that name. It serves under the authority Android attached it with
  (`attachInfo`), never one it computes.
- `FluiCallback` is the one handler of every `java.lang.reflect.Proxy` the crate makes for a
  listener interface (a dialog's three listeners): JNI defines no class, and a handler is an
  object of a Java type. A call reaches the native `called` with the method's name; once the
  object is collected, a phantom reference hands its handle to `released` from a daemon thread of
  the class (a `Cleaner` does that from API 33, above the floor).

Their dex is checked in (`android/classes.dex`, compiled by the `Android dex` workflow and held to
the sources by `tests/android_dex.rs`) and declared with the sources, the R8 rules and the
provider in `[package.metadata.flui.declares]` (ADR-0307), so `flui build` packs it beside the core host's and writes the `<provider>` element;
no user's build runs `javac`. Only the providers that call the classes read them, once per runtime:
the launcher and the share sheet (`FluiDocuments`) and the message box (`FluiCallback`). The
library reads `FluiCallback.PROTOCOL` before it binds anything, and an APK without the classes,
with another protocol, or whose manifest declares no `FluiDocuments`, refuses those three with
`NotDeclared`; the file dialogs, the user's folders and the system information call Android's
classes alone and work without them. No compiler checks the names and signatures the JNI calls
spell: they were read against the platform's `android.jar` (typed bindings are the direction,
[`docs/gaps.md`](../../docs/gaps.md#jni-calls-are-hand-written-signature-strings)).

Threads:

- **The owner thread is not Java's main thread.** The providers reach Android through the Activity
  the host lends (`ProviderContext::activity`, ADR-0154 §9): a runtime whose host offers none (the
  headless host) is refused with `NotYetOnThisHost`. The library keeps no `JavaVM` of its own: the
  first provider of a UI runtime reads the VM, the class loader and the application's content
  resolver in a frame the Activity lends, and keeps them with the runtime.
- **Starting an activity and the message box run on Java's main thread**, each work the host runs
  there for the provider (`Activity::on_main_thread`, ADR-0377 §6) inside its panic boundary. Work
  the host refuses (the Activity ended, `MAX_POSTED_WORK` pieces wait, or the main thread did not
  take it within the host's limit) answers a launch or a share `Unavailable`, and a message box
  `NotShown`. A dropped box's dismissal is owed: refused, it is tried again when the refusal
  arrives, until it lands or the Activity ends (`dismiss_until_landed`), and the refusal is logged
  once. The host answers `TooMuchPosted` only once a place has freed and `HandOffFailed` at its
  deadline, so the retry never spins the owner.
- **Whatever reads a content provider or the file system runs on a thread of its own**
  (`android::workers`), at most `Workers::MAX_IN_FLIGHT` per runtime; a request past that answers
  `Unavailable`, and a worker gives its place back on every exit. A provider answers when it will
  and may ignore a cancellation, so a worker can stay inside a call.

Both native methods are one call of `owner::android::native_call::jni_native`, the crate's adapter
of the same contract as `flui-native`'s: the body runs inside the panic boundary, and no Java
exception is left pending when the method returns.

**File dialogs** are the Storage Access Framework's pickers, started through
`Activity::launch_for_result` under a key per kind. The picker filters by MIME type alone: a
filter's MIME types pass as they are and each extension as the type `MimeTypeMap` maps its last part
to; an extension with no type, or more types than `MAX_INTENT_TYPES`, shows every document, and the
picks are held to the filters all the same (ADR-0360 §4). A pick is a content URI, so it is handed
over as its name and contents (below). A save carries its contents and is written through the URI
the user chose (`wt`, or `w` where the provider cannot truncate); the document is typed by the
proposed name's extension, else by the first filter's, so Android's own provider adds that
extension to a name the user types without it, and a type with no extension leaves the name as
typed. A document that could not be written is deleted. A folder dialog answers `NotYetOnThisHost`
and a save without contents `NotOnThisPlatform`; the picker shows no title, accept label or
starting folder. The dialog's hold is the request's `ResultTicket`: dropping it before the answer
finishes the picker (ADR-0344); dropping it after the answer arrived and before its owner turn
leaves the answer unread; dropping it while a worker reads cancels the provider's calls that take
the `CancellationSignal` (a query, an open) and stops the worker before its next document and
between two `READ_CHUNK`-sized reads, though a read the provider holds blocked inside one chunk
keeps the worker's place until the provider writes or closes. A save the user has answered
completes whoever still waits. When Android replaces the Activity the request ends
`ActivityEnded`, answered as the window closing; the answer that comes later is claimed by the next
runtime's first dialogs (`Activity::reattach` under each key), so the host holds nothing: an open's
answer is let go and the empty document of a save deleted.

**Launching.** A URI is `ACTION_VIEW` as written under `CATEGORY_BROWSABLE`, with no grant: only an
activity that declares itself reachable from a link takes it. A document never leaves as a path or
a `file:` URI: it is opened on a worker without blocking and without taking a terminal, what the
descriptor holds is judged (a regular file whose final name, read from `/proc/self/fd`, neither
`runs_when_opened` nor installs a package), and the open file is handed to `FluiDocuments`, which
serves it under `content://<authority>/<random token>/<name>` (the authority the manifest gave it,
`<application id>.flui.documents`) to the one activity the intent's read grant reaches (ADR-0360
§2). The provider is not exported, serves only what was offered, and reopens the file through the
descriptor it holds (never the path). An offer is opened and queried as often as its grantee
asks (the share sheet opens an image for its thumbnail before the chosen application does)
until its start does not happen (the `Intent` failed to build, or Android refused the start),
when the library withdraws every offer the start made (`FluiDocuments.withdraw`,
`StartAnswer::keeps_offers`), or `FluiDocuments.OFFER_LIFETIME_MS` passes, which is also what
ends the offers of a share whose chooser the user dismissed, since Android reports no
dismissal; a grantee that opened it keeps its own descriptor after that. At most
`FluiDocuments.MAX_OFFERED` are served at once; one more is refused rather than replacing an
offer whose grant may be in use, classified from `offer` alone, and answers the launch or the
share `Failed(Os(EMFILE))` (`TOO_MANY_OFFERED`). A file unlinked between its open and the read of
its final name is refused by the `fstat` already made (`Held::Unlinked`), not judged by the
`(deleted)` name the kernel gives it.

**Sharing** is `ACTION_SEND`, or `ACTION_SEND_MULTIPLE` for several files, through the chooser: the
text and the link travel as `EXTRA_TEXT`, the link on its own line, the title is the chooser's and
the subject. Each file is opened on a worker as a launch's document is (`android::opened`: without
blocking, without taking a terminal, judged by what the descriptor holds) and shared only when it
is a regular file, under its final name (`share::open_shared`), so a link named `notes.pdf` that
leads to `secret.db` goes out as `secret.db`; unlike a launch, any name is shared, since a share
hands over contents. The intent's clip carries the grant to the target the user picks.

**The message box** is an `AlertDialog` whose listeners are one proxy; it answers once, with the
first of a button, the user leaving (a back press or a touch outside, which answers what closing
means for the buttons; a Yes or No box cannot be left) or a box that could not be shown. The hold
takes the caller's answer away and dismisses the box, or keeps one not yet shown from showing. The
buttons are labelled in English: Android's `yes` and `no` strings read "OK" and "Cancel".

**The user's folders** are `Environment`'s public directories (Documents, Download, Music, Pictures,
Movies), which it names without reading storage; Android has no home folder, desktop, templates or
public share. **The system information** is the system properties' release and ABI.

## Picks and saves without a path

- A pick is a `PickedFile`, not a path, because not every system answers one; `PickedFile::form`
  says which it is (`PickedForm`). A desktop dialog answers where the file is (`Path`); a browser
  answers a file a page may read but never a path, iOS a document the application may reach only
  while it holds the access its picker granted, and Android's picker a content URI, so there a
  pick carries its name and contents (`Contents`), read when the dialog answers (the checks such a
  host makes are one set of rules, `dialog::handed_over`); a save handed to the browser as a download
  is `Download`, whose place and fate the browser does not report (it may be cancelled, blocked or
  renamed), so it carries the name it was offered under and no contents. Reading is bounded by the
  request (`OpenRequest::max_bytes`, by default `TransferLimits`' default, a drop's bound): the
  picks' declared sizes are checked before a byte is read and the bytes read are counted again,
  and more is `DialogError::TooLarge`. Each file is copied once, from the browser's buffer or the
  document into the pick's own.
- A folder a browser hands over is `PickedForm::Folder`: its name, and the files under it at any
  depth (`FolderFile`), each where it is under the folder and what it holds, in the order of
  their paths. It is read whole when the dialog answers, as a handed-over file is, since a
  browser gives a page one of two things for a folder and only a list of files is common to
  both: a handle to walk, or the files with their paths. So files are all the form carries, and a
  folder that holds none, at any depth, leaves no trace in it. A file's place is a `FolderPath`:
  the names from the folder down to the file, each held to one rule on every host (not empty,
  not `.` or `..`, no `/`, `\` or NUL), so a path has one name per folder passed and none that
  climbs; any other name a file system allows is kept, a blank one or one with a control
  character among them. It is not a path of the host, and promises nothing of one made from it
  (Windows reads `NUL` and `C:` its own way and drops a trailing dot): an application that
  writes the files to disk maps the names first. A folder is listed and checked once, before
  its files are read (`FolderListing`: its name one a folder holds, no two files at one path,
  the files in the order of their paths), and each file stays beside its path until the pick
  is made of them. The folder is bounded twice: by `OpenRequest::max_entries` files
  (`DEFAULT_MAX_ENTRIES` unless the request says, past it `DialogError::TooManyEntries`), and
  by `max_bytes` for its files together; the handle holds every backend's answer to both, and
  a backend that walks the folder counts the folders it meets as well, so the walk ends at
  the bound. A desktop dialog answers a folder's path, which neither bounds.
- Every pick is held to the request's filters, and one that matches none refuses the whole answer
  with `DialogError::NotInFilter`: the answer is what the user chose, and it is not what the
  request asked for. A pick's name is the handle's to check, for every backend (`FileDialogs::open`):
  every system's dialog lets a user past its filters somewhere (a name typed into it, a link whose
  target has another name, a type that only conforms to an allowed one, a browser's input that
  offers every file beside them). The handle cannot tell a file's type, so it checks a request
  whose filters name extensions and file names only, and a backend that mapped a MIME pattern
  judges the pick by its type (`FileFilter::matches` with the browser's type on the web and the
  provider's on Android, type conformance on macOS and iOS). An iOS or Android pick is judged
  before it is read, so one the filters do not mean is never read. A request that offers a filter
  of every file admits every pick (ADR-0374 §3).
- A save may carry the contents it writes (`SaveRequest::contents`), the one way to save where
  the system hands over no file to write later (a page, iOS). The handle writes them to a path the
  user chose on a thread of its own (`dialog::write`) and answers once they are written (a failed write
  answers the system's code); a backend that hands over no path writes them itself and answers the
  file it wrote. Once the user has chosen, the write completes even if the caller let the dialog
  go, on every host. A save without contents answers where to save, and on the web, iOS and
  Android, which have no path to answer, is refused with `NotOnThisPlatform`. Access to a pick
  that outlasts the process is a grant (below).
- The contents replace the chosen file whole or not at all: they go into a new file beside it,
  synced, which takes its name, so a failed write leaves the chosen file as it was. A chosen link
  is written through: the new file is made beside the file the link leads to and takes that
  file's name, so the link stays a link. On Unix the new file is made with the mode of the file it
  replaces, before anything is written to it, so the contents are never readable by more users
  than the old file's; its owner, access control list and extended attributes are its own. Only a
  regular file is replaced; a device or a pipe is written into.
- The contents go into the chosen file itself only where the file system refuses the new file by
  its design, classified by the error's kind and code (`dialog::write`): it cannot be made beside
  the chosen one (a folder that takes no other name, as a sandbox's document folder; a read-only
  folder holding a writable file; a name too long to make a sibling's from), or it cannot take the
  chosen file's name because the two are on different devices (`EXDEV`,
  `ERROR_NOT_SAME_DEVICE`), the chosen file is a mount point (`EBUSY`) or another user's in a
  sticky folder (`EPERM`). That write is not whole or nothing: it empties the file first, so one
  that fails midway, or a process killed during it, leaves the file cut short. Any other failure
  answers the system's code with the chosen file as it was and the new file removed: a volume out
  of room, a read-only file, a file another process holds open without letting it be replaced (on
  Windows, open without `FILE_SHARE_DELETE`), where writing into it would empty a file someone
  else is using.

## Grants (`grant`, `sandbox`)

The decision is [ADR-0380](../../docs/adr/ADR-0380-a-pick-carries-a-grant-that-outlasts-the-process-and-no-reacquired-path-is-one-the-sandbox-keeps-closed.md);
what is not built is in [`docs/gaps.md`](../../docs/gaps.md#sandbox-aware-file-access-is-partial).

- An open that `keep_access` hands each pick over with a `Grant`, the system's lasting access
  in its own form, tagged by its `GrantKind` and written as bytes behind a versioned header. A
  pick the system grants nothing lasting for carries none; a web pick never does.
- `Grants` is not bound to a window: an application reopens its last document before any window
  shows. `resolve` refuses a grant its backend does not take (`GrantBackend::takes`: another
  kind of system's, or the scripted double's) with `Foreign` without asking it, and holds every
  answer to the process's `Sandbox`: a path the sandbox does not reach and no started
  `AccessScope` holds open is `Revoked`, contents past the request's bound `TooLarge`, a folder
  of files, a download or a fresher grant of another origin `MalformedAnswer`. Only the crate
  makes an `AccessScope`: a backend outside it answers `Access::new`, held to the sandbox.
- A resolve lasts while its `Resolving` is held; dropping it delivers nothing. One still waiting
  for the sandbox leaves the queue; one the backend runs drops its `ResolveHold`: on Android the
  read's let-go is set and its signal cancelled, so the worker stops before its next chunk and
  its place comes back (a read that finished cancels nothing); on macOS and iOS the resolution
  finishes but starts no scope (`Waited::hold`, the resolve's whole wiring, tested on every
  host); a path's `metadata` call cannot be called back, and its answer
  is dropped.
- The sandbox is `flui_platform::sandbox`'s detection, the one the data folders use: a handle
  asks its backend (`GrantBackend::sandbox`) one read at a time, resolves wait for the answer (at
  most `Grants::MAX_WAITING`, one more refused `Unavailable`), and a read answered with nothing
  is asked again at the next resolve. A backend that panics asking leaves the slot unread: the
  read runs under `FirstPanic` (re-exported by `flui_platform::capability`, since a capability
  crate depends on the contract alone), every resolve waiting is refused `Unavailable`, and the
  panic is resumed after (ADR-0287). The portal reads `/.flatpak-info` on its pool (a
  manifest's `xdg-*` entries through `user-dirs.dirs`, the user's home from the user database),
  and a portal that cannot start, or a description it could not read, answers nothing, so the
  next resolve asks again; macOS reads the App Sandbox from the environment and the user
  database;
  iOS's container is `HOME` with a `TMPDIR` inside it; Android's reaches no path, since every
  pick and reacquired document there is its contents; Windows has none.
- A path grant is judged before any file system call: a relative path is `MalformedAnswer`, a
  share, a pipe or a device `NotLocal` (`flui_platform::launch::is_local_path`, ADR-0360 §2).
- Per host: macOS makes a security-scoped bookmark of each pick on the work queue once the panel
  answered (a queue with no place answers the picks without grants) and resolves it there with
  its scope, which the `Access` stops when it drops, a stale one made again while the access is
  held; iOS makes a minimal bookmark under the picker's access as the document is read; Android
  reads and bounds each pick's URI, then persists its read permission, once every pick was read,
  giving a permission back when no grant can hold its URI, and reacquires the document on a
  worker as its contents, refused `Revoked` once the permission is no longer held; Linux and
  Windows keep the path and ask the file system on the portal's pool or a shell thread, a file
  gone from the document portal's folder answering `Revoked`, one gone elsewhere `Missing`.
- The scripted double (`testing::ScriptedGrants`) resolves from a `GrantStore` that outlives the
  providers, as a system's permission store outlives the process, in the `Sandbox` the test set:
  a restart is a new host over the same store. Its grants stand in for the host's kind of grant
  and carry a tag outside every `GrantKind`, which only a build with the double reads and no
  system's backend takes (`Grant::made_by`); `Grant::kind` always names a system, with the
  feature or without it. The double can answer, drop, hold or panic at the question for the
  sandbox.

## The web providers (`web`)

- Each provider reaches its page through the acquiring widget's window, from the canvas its raw
  handle names (`web::page::Page`), never through the browser's globals; a widget in no window,
  or in one that is not a canvas, is refused with `NoWindow`. `ProviderContext::app_id` and
  `WindowRef::portal_parent` mean nothing to a page and are not read.
- What the providers decide without the DOM (the filters' mapping and checks, a save's name, a
  launch's route, what an exception means) is `web::rules`, tested on every host; the DOM side is
  `web::tests`, the lib test on wasm32 in headless Chrome (CI's `wasm-check` job), with user
  activation stated by the test and `window.open`, the save and directory pickers and the Badging
  API replaced, so no picker or window opens.
- **User activation, one answer.** A browser shows a picker or a new window only while the page
  handles a user's gesture. A call outside one is refused with `Unavailable` before the browser is
  asked (the browser would refuse it, or a file input and a popup blocker would drop it
  silently), as `flui-native`'s web vibration is, and the browser's own refusal of such a call
  (`NotAllowedError`, `SecurityError`, a blocked window) is answered with the same `Unavailable`;
  a browser without `navigator.userActivation` is asked and refuses itself.
- **Open.** The File System Access picker where the browser has it (Chromium, in a secure
  context) and can express the filters (each extension of ASCII letters, digits, `+` and `.`, at
  most 16 characters with its dot, Chromium's `VerifyIsValidExtension`); `<input type=file>`
  otherwise, its `accept` the filters' union. A filter of every file, or one that names a file
  exactly, keeps the picker's own entry of every file (`excludeAcceptAllOption` false) and empties
  the input's `accept`, since neither names a file by its whole name; the picker's types are the
  filters that have an extension or a MIME type (ADR-0374 §4), and the handle holds each pick to
  the filters. A picker the document may not show (`SecurityError`:
  a cross-origin frame, which may show an input) hands over to the input while the page still
  handles the gesture, and answers `Unavailable` once the gesture is spent. A dismissed picker
  answers `AbortError`. An input ends on its `change`, its `cancel` (Chromium 113, Firefox 91,
  Safari 16.4) or `pagehide`. Only where the engine fires no `cancel` does it also end when the
  window regains focus with no `change` within `web::dialogs::FOCUS_GRACE_MS`, which answers a
  cancel: there a pick reported later than that (a large file a phone copies first, a folder the
  browser lists first) is lost, the price of an end at all. An engine that fires `cancel` is
  never timed. No API says whether the event is fired, so `Page::input_fires_cancel` reads
  `Headers.prototype.getSetCookie`, which came with Chromium 113 and after the event elsewhere
  (Firefox 112, Safari 17): a Firefox before 112 or a Safari before 17 keeps a bound it does not
  need.
- **Folder.** The directory picker (`showDirectoryPicker`, asked to read) where the browser has
  it, its handle walked through each folder's `values()`; `<input type=file webkitdirectory>`
  otherwise, and in the picker's place under the rules of an open (the gesture, `SecurityError`,
  one picker per page, the input's ends), its files' `webkitRelativePath`s naming the folder and
  where each file is under it. Both answer one folder as `PickedForm::Folder`, whatever the
  request allows: a browser's chooser takes one. The folder is listed once and held to what a
  pick carries, the walk's entries to `max_entries` as they are met, an input's files counted
  before one of them is looked at, and the files' declared sizes to `max_bytes`, all before a
  byte is read. A browser with neither API answers
  `NotOnThisDevice` before the gesture is asked about, since no later call fares better. An
  input may take the attribute and show a file chooser all the same (Chrome for Android before
  132, Safari on iOS before 18.4), or pick a folder without saying where its files are (Firefox
  for Android 141): its files then all have an empty path, which is answered `NotOnThisDevice`
  too. An input answers no file for a folder that holds none, which it cannot tell from a
  dismissed chooser: a cancel.
- **Save.** The save picker writes the request's contents through a writable stream, aborted on
  a failure, after the user chose even if the dialog was dropped: the browser has made or emptied
  the chosen file by then. Without a picker, outside a gesture (which the picker needs and a
  download does not), or when the browser refuses the picker for want of either, the contents are
  handed to the browser as a download under the suggested name, completed by the first filter as a
  typed name is on the portal (`dialog::save_file_name`), from an object URL revoked a minute later, and answered as
  `PickedForm::Download`. The answer reports no filter: the browser says none was selected.
- **One picker per page.** A page cannot close a browser's picker, and the browser shows one at
  a time. While one is up the document carries a mark (`web::dialogs::ShownPicker`, an expando
  as the message box's id counter is), so a second is refused with `Busy` whichever window or
  UI runtime of the page asks, and after its dialog was dropped: a picker of the API until its
  promise settles, an input until it ends as above, keeping itself alive until then.
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
- **The message box** is a modal `<dialog>` beside the canvas, each button titled by the request's
  label or its English word, the request's default button focused (`autofocus`, so Enter presses
  it), and its detail a paragraph below the message. A box without a title has no heading. A
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
  against `SystemPlugin` on the host it runs on (the Windows shell, AppKit or the Linux portal),
  asking nothing a user answers. Against the platform's providers it proves acquisition, the seam's
  refusals and the launcher entry's shared state only (its windows are headless), and the system's
  name and home folder. `tests/grants.rs` runs the grants on a simulated sandbox: the layer's rule,
  a restart over the same `GrantStore`, a revoked, deleted, stale and foreign grant.
- `win32::*::tests`: each Windows provider at its OS seam, with the test owner
  (`testing::owner`) and the stand-ins of the crate's kit (`testing::win32`, test builds on
  Windows only); `win32::modal::tests` and
  `win32::dialogs::tests` run the dialog thread and its dismissal window with a stand-in dialog.
- `tests/portal.rs` and `tests/portal_services.rs` (Linux): the portal providers against a
  stand-in portal on a private bus.
- `macos::*::tests`, `ios::*::tests` and `apple::*::tests` (every host): each macOS and iOS
  provider's rules as a model, with the system's answers
  as inputs and `testing::owner::TestOwner`'s turns; the AppKit calls compile only for macOS, the
  UIKit calls only for iOS.
- `android::*::tests` (every host): each Android provider's rules, and in `android::documents::tests`
  the file dialogs from a request to its answer over the kit's stand-in Activity and picker
  (`testing::android`): the Activity's refusals, a dialog let go, a save that completes, an earlier
  Activity's answers claimed. `a_launch_judges_the_file_it_opened_by_its_final_name` opens real
  files and links on Linux, and `the_java_classes_state_what_the_library_and_the_manifest_name`
  holds the Java sources to the constants the library and the manifest state. The JNI calls
  compile only for Android.
- `web::rules::tests` on every host, and `web::tests` in headless Chrome (CI's `wasm-check` job
  runs `cargo test -p flui-system --lib --target wasm32-unknown-unknown`).