# flui-system architecture

The OS shell services, a capability crate (ADR-0154 §11): each service is a marker, a handle
and a backend trait in this crate, a provider per platform, and a scripted backend under
`testing`. What the code does not show:

## Handles

- Every handle is owner-thread and answers through a `Completion` that settles at an owner
  turn, never inside the call that asked. A request the handle can tell is malformed is refused
  before any backend is asked, with a typed reason (`MalformedRequest`, `MalformedAutostart`,
  `MalformedShortcut`).
- A request its caller can call off ends when its value drops: a `Dialog` (ADR-0344) and a
  `ShortcutBinding`, each through the hold its backend returned.
- `GlobalShortcuts` delivers a stream through `ShortcutFeed`, the one cross-thread queue here:
  at most `ShortcutFeed::MAX_QUEUED` presses and releases (past it the oldest goes, counted by
  `ShortcutBinding::dropped`), one whole binding (a newer `Bound` or `Changed` takes the place
  of the one queued) and the end, which is answered again after it.
- `TaskbarEntry` is the application's: the handles of one UI runtime share one state, a change
  that changes nothing is not sent, and the backend is always handed the whole state.
- Message box and share have no contract yet: Linux has no portal for either, so each comes with
  its first provider, and the Linux provider then refuses it with `NotOnThisPlatform`.

## The Linux providers (`portal`)

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
- Autostart, global shortcuts and the user folders answer `NotOnThisPlatform`. Message box,
  share and the jump list come with their contracts.
