# ADR-0364: A window names itself to a desktop portal for each request

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0152](ADR-0152-capability-seam-revised.md) §2 item 6 in part (`WindowRef`
  also answers the window's portal parent; `ProviderContext` also names the application id);
  [ADR-0154](ADR-0154-capability-crates.md) §9 items 1 and 5 in part (a portal takes the parent
  the host fills per request, not one built from the raw handle)
- **Related:** [ADR-0344](ADR-0344-a-dialog-lasts-while-its-caller-holds-it-and-ends-with-its-window.md)
  (a dialog ends with its window), [ADR-0351](ADR-0351-a-linux-window-and-its-desktop-entry-carry-the-application-id.md)
  (the application id a Linux window carries), [ADR-0154](ADR-0154-capability-crates.md) §5 and
  §11 (Linux takes portals; `flui-system`), [ADR-0254](ADR-0254-window-contract.md) (the window
  contract)
- **Sources:** XDG Desktop Portal, "Window identifiers" and the `Request`, `Session`, `Background`,
  `GlobalShortcuts` and `org.freedesktop.host.portal.Registry` interfaces
  (`flatpak/xdg-desktop-portal`, `data/*.xml`, `desktop-portal/background.c`); the
  `xdg-foreign-unstable-v2` protocol; GTK 4's `gdk_toplevel_export_handle` and
  `gdk_toplevel_unexport_handle`, which export a toplevel for one portal call and release it after.

## Context and problem statement

A portal shows its dialog attached to the window named by `parent_window`: `x11:<XID>` for an X11
window, `wayland:<handle>` for a Wayland window the compositor exported through `xdg-foreign`.
The capability seam handed providers a `WindowRef` with the window's raw handles only, so
`flui-system` derived `x11:<XID>` from the X11 handle and sent an empty parent for every Wayland
window: its dialogs and application choosers were not modal there. The Wayland host exported each
window as it opened and kept the export for the window's life, and only tests read the handle.

An export held for the window's life names the window to any client that learns the handle, for
as long as the window lives; the handle a request needs lives only as long as the request. The
portal also attributes a request to an application: inside Flatpak or Snap the sandbox names it;
outside one the portal knows it only if the application named itself through
`org.freedesktop.host.portal.Registry` before its first portal call, and the background portal
refuses autostart to an application it cannot name. No provider knew the application id the
composition root parsed (ADR-0351).

How does a provider name the window to a portal, and the application?

## Decision drivers

- One window abstraction for providers, no OS extension trait: the portal parent is the seam's
  notion, filled by each host (the owner's decision of 2026-10-28).
- The export lives exactly as long as the request that needs it, and is released when the request
  ends, however it ends (answered, dismissed, the window closed, the UI runtime retired).
- The X11 answer is immediate; the Wayland answer comes with the compositor's event: one shape
  takes both without blocking the owner thread.
- One identity, parsed once (ADR-0351): providers take the composition root's `AppId`.

## Considered options

1. `WindowRef::portal_parent(Resolver<PortalParent>)`, answered by a new
   `PlatformWindow::portal_parent`; `PortalParent` holds the export and releases it on drop; the
   Wayland host exports per request. `ProviderContext::app_id()` from the host bridge.
2. The window keeps one export for its life and `WindowRef` hands out its string.
3. A Linux extension trait on the window that a provider downcasts to.

## Decision

Chosen option: 1, because it is the only one whose export ends with its request, and it keeps
the notion in the seam.

1. **`flui_platform::capability::PortalParent`** is the `parent_window` a request passes and what
   keeps it valid: `none()` (the empty string), `x11(NonZeroU32)` (`x11:<hex>`), and
   `wayland(handle, export)` (`wayland:<handle>`), whose `export` (`Any + Send`) is dropped with
   the value. It is `Send`: a request ending on the portal's thread drops it there.
2. **`PlatformWindow::portal_parent(&self, Resolver<PortalParent>)`** names the window for one
   request. An X11 window answers `x11:<XID>` at once. A Wayland window calls
   `zxdg_exporter_v2.export_toplevel` for the request and answers when the compositor's `handle`
   event comes, the answer holding the `zxdg_exported_v2`, whose drop destroys it and flushes the
   connection. **The window's close releases every export a request still holds**, so no export
   outlives its toplevel; the request goes on without a parent, which the portals allow. A
   compositor with no exporter, and a closed window, answer `none()`. The default
   answers `none()`: only a Linux desktop has portals. The window no longer exports itself as it
   opens.
3. **`WindowRef::new(&Rc<dyn PlatformWindow>, WindowLifetime)`** makes the reference from the
   window (its id, scale and surface as before) and keeps a `Weak` to it.
   **`WindowRef::portal_parent(Resolver<PortalParent>)`** answers `none()` for a closed window or
   one its host let go, else asks the window. No other window method becomes reachable.
4. **A provider holds the parent until its request ends**: `flui-system` moves it into the
   request's task and drops it when the portal answered, the request was closed (the caller
   dropped it, a dialog's window closed, the UI runtime retired), or it failed; whatever the
   request, its export goes with its window (item 2). A dialog that ends
   before its window was named shows nothing.
5. **`ProviderContext::app_id() -> Option<AppId>`** names the application, from
   `HostBridge::name_application`, which the desktop composition root calls with the id its
   windows carry. `flui-system` names an unsandboxed application to each portal instance through
   the registry before the connection's first call to it, and again to a portal that restarted;
   inside Flatpak or Snap it does not.

### Consequences

- Good, because a dialog, an application chooser, an autostart question and a shortcut binding are
  modal to their Wayland window, and no handle names the window between requests.
- Good, because the portal attributes an unsandboxed application's requests to it, which the
  background portal needs to write its autostart entry.
- Bad, because a Wayland request waits one round trip to the compositor before it reaches the
  portal.
- Neutral, because a window of any other system answers `none()`, and `WindowRef::new` now takes
  the window rather than its parts.

### Confirmation

The compiler: `WindowRef::new` takes the window; `PortalParent` keeps its export private. Tests:
`a_portal_names_each_kind_of_window_and_a_wayland_name_lasts_with_its_export` (flui-platform),
`each_portal_request_exports_the_window_and_releases_the_export_as_it_ends` and
`a_compositor_without_an_exporter_names_no_portal_parent` (flui-native, against the scripted
compositor), `a_portal_parent_names_the_window_while_its_request_holds_the_export` (flui-native,
under a headless sway), `a_request_names_the_window_by_its_export_and_releases_it_when_it_ends`,
`a_launch_names_the_window_by_its_export_until_the_portal_answers` and
`the_application_is_named_to_each_portal_once_before_its_first_call` (flui-system, against the
stand-in portal).

## Pros and cons of the options

### 1. A parent per request, through the window

- Good, because the export's life is the request's, and one shape answers X11 at once and Wayland
  after its event.
- Bad, because `PlatformWindow` grows a method and `WindowRef` holds a weak reference to the
  window.

### 2. One export for the window's life

- Good, because the handle is ready before any request.
- Bad, because the handle names the window to any client for as long as the window lives, and a
  request cannot release what it did not take.

### 3. A Linux extension trait

- Good, because no other host sees it.
- Bad, because the portal parent is the seam's notion, and every provider would downcast to an OS
  type the seam keeps out of capability crates.

## More information

A run against GNOME's and KDE's portals, inside and outside Flatpak, is owed and recorded in
`crates/flui-native/platform-parity.toml`.
