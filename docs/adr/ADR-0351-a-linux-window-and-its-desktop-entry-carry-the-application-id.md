# ADR-0351: A Linux window and its desktop entry carry the application id

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md) §4 for the
  Linux desktop entry's condition only (the entry is written for every Linux build, not only when
  the application opens a URL scheme or a file type);
  [ADR-0254](ADR-0254-window-contract.md) §4's clause that an app id outside reverse DNS is warned
  about at `start_single` only; [ADR-0327](ADR-0327-the-native-host-is-owner-local-and-runs-its-io-on-the-runtime-its-creator-hands-it.md)
  §3's signature of `CurrentHost::start_with` only (it also takes the application id)
- **Related:** [ADR-0254](ADR-0254-window-contract.md) §4 (one instance per session, named by the
  application id), [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (shared code moves down a
  tier), [ADR-0082](ADR-0082-platform-api-contract-crate.md) (only `flui-app` starts a host)
- **Sources:** the freedesktop Desktop Entry Specification 1.5 (file name of a desktop file ID,
  `StartupWMClass`); ICCCM §4.1.2.4 (`WM_HINTS`) and §4.1.2.5 (`WM_CLASS`); the `xdg-shell`
  protocol (`xdg_toplevel.set_app_id`); `rust-windowing/winit` `WindowAttributesExtWayland::with_name`;
  GTK 4's `gdk_x11_surface_set_utf8_property` (`_GTK_APPLICATION_ID`).

## Context and problem statement

A Linux shell matches a window to its desktop entry, and with it the name, the icon and the
launcher the window groups under, by the Wayland toplevel's app id or the X11 window's `WM_CLASS`,
held against the entry's file name and its `StartupWMClass`. GNOME Shell and KWin also tie an
activation token to the application that launched it. FLUI's windows carried no app id on Wayland
and the executable's stem on X11, though the process knew the application id (`FLUI_APP_ID`, which
named its storage directory and its single-instance bus name); `flui build linux` wrote a desktop
entry only for an application that opened a URL scheme or a file type, and without
`StartupWMClass`. The identifier type lived in `flui-log`, which only composition roots may link,
so the host took the id as a `&str` it checked again with a grammar of its own, and warned and
carried on when the check failed. How does one identity reach the window and the entry?

## Decision drivers

- One identity, parsed once: every consumer takes the same checked value, never a string.
- The host stays below the composition root: `flui-native` cannot link `flui-log`.
- An id a desktop entry cannot be named by is refused where it is made, not rewritten.
- The window and the entry must match in every build, an application without `flui.toml` too.

## Considered options

1. `AppId` moves to `flui-foundation`; the host takes `Option<&AppId>` at its start and the Linux
   windows carry it; the CLI writes the entry for every Linux build with `StartupWMClass` and
   compiles the entry's id in.
2. The host keeps a `&str` and its own check; only the windows change.
3. A Linux launch configuration type in `flui-platform` with its own identifier field.

## Decision

Chosen option: 1, because it is the only one with a single checked identity and no second parser.

1. **The identifier is `flui_foundation::AppId`.** Its grammar (reverse DNS, labels of ASCII
   letters, digits and `_` led by a letter, no Java word, no Windows device name first) is a D-Bus
   well-known name and a desktop-file ID. Its longest form is `APP_ID_MAX_LEN` = 255 bytes less
   `.desktop`, so `<id>.desktop` is a file name; a longer id is `AppIdError::TooLong` at
   `AppId::try_new`. `flui-log` re-exports it; `IdentityError` keeps the display name's refusals.
2. **A host is started with the id.** `CurrentHost::start_with(executor, Option<&AppId>)` and
   `CurrentHost::start_single(executor, &AppId)` take the parsed id; the single-instance name is
   the id itself, with no second check. `flui-app` passes `AppIdentity::app_id`.
3. **A Linux window carries it.** The host decides its `AppClass` once at start
   (`window::linux::class`): the id, or without one the executable's stem. A Wayland window calls
   `xdg_toplevel.set_app_id` before its first commit; an X11 window writes `WM_CLASS` with the
   class as both instance and class, `_GTK_APPLICATION_ID` when it is an id, and `WM_HINTS` with
   `InputHint` set and input `True`.
4. **The entry matches.** `flui build linux` writes `share/applications/<id>.desktop` for every
   build, with `StartupWMClass=<id>` and `MimeType` only when the application opens something,
   and compiles the same id in as `FLUI_APP_ID` (an application without `flui.toml` has it from its
   directory).

### Consequences

- Good, because the dock shows the entry's name and icon for the window and a launch's activation
  token is tied to the right application.
- Good, because an invalid id cannot reach a host: the host's own grammar and its warning path are
  gone.
- Bad, because an identifier between 248 and 255 bytes, accepted before, is refused.
- Neutral, because the other hosts take the application's identity from their package and ignore
  the id; a Windows AppUserModelID is not set (a process id that no shortcut carries would break
  pinning), which `platform-parity.toml` records.

### Confirmation

The compiler: `start_with` and `start_single` take `AppId`, which only `try_new` makes. Tests:
`an_app_id_is_a_reverse_dns_name_every_platform_accepts` (flui-foundation),
`a_window_is_classed_by_the_application_id_or_the_executable` (flui-native, pure),
`a_window_carries_the_application_id_it_was_started_with` (flui-native's `x11_window` on Xvfb with
openbox, and `wayland_window::optional` under a headless sway, which lists toplevels to another
client), `the_entry_and_the_window_are_named_by_one_identifier` and
`a_linux_desktop_registers_what_the_application_opens` (flui-cli).

## Pros and cons of the options

### 1. `AppId` in `flui-foundation`, taken by the host

- Good, because one type is parsed once and named by the log, the storage host, the host and the
  application alike.
- Bad, because it moves a public type and splits `IdentityError`.

### 2. The host keeps a string

- Good, because it touches only the windows.
- Bad, because two grammars stay alive, and an id one accepts and the other refuses is a warning
  at run time.

### 3. A launch configuration in `flui-platform`

- Good, because it would carry later launch facts.
- Bad, because it needs its own identifier type or a second parse, and nothing else rides on it
  yet.

## More information

The desktop entry is still not installed by FLUI: a package installs it. A run under GNOME and KDE,
where the shell's matching is what a user sees, is owed and recorded in
`crates/flui-native/platform-parity.toml`.
