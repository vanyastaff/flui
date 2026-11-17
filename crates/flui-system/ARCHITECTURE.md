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
  bounded at `ShortcutFeed::MAX_QUEUED`, a press or release dropping the oldest press or release
  first, never a `Bound`, a `Changed` or the end, which is answered again after it.
- `TaskbarEntry` is the application's: the handles of one UI runtime share one state, a change
  that changes nothing is not sent, and the backend is always handed the whole state.
- Message box and share have no contract yet: Linux has no portal for either, so each comes with
  its first provider, and the Linux provider then refuses it with `NotOnThisPlatform`.

## The Linux providers (`portal`)

- One `Portal` per UI runtime (`ProviderContext::runtime_local`): a thread of its own runs a
  `zruntime` executor; every request is a task there, and every file-system call (opening a file
  for the portal, reading `user-dirs.dirs`, reading `/proc/self/exe`) runs there, never on the
  owner thread.
- A request reaches the portal through `Bus::desktop`: the connection, the portal's current
  unique owner (an installed portal that is not running is started first), and, for an
  unsandboxed application with an id, the application named to that owner through
  `org.freedesktop.host.portal.Registry`, once per portal instance and before its first call.
  Every call goes to that owner, and only signals that owner sends are heard.
- Before a request depends on an interface it reads the interface's `version`
  (`Desktop::require`); an absent interface or an older version is `NotOnThisDevice`. A portal
  that is not on the bus is `Unavailable`.
- A request that shows something names the acquiring widget's window: `with_parent` asks
  `WindowRef::portal_parent` (ADR-0364) and the task holds the `PortalParent` until the request
  ends, so a Wayland window's export lasts exactly as long. A request with no window names none.
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
  `application://<id>.desktop`; an application with no id has none (`NotDeclared`). A burst of
  changes sends the latest state once.
