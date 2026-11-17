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
- An id a name it is part of cannot hold is refused where it is made, not rewritten.
- The window and the entry must match in every build, an application without `flui.toml` too.
- One application has one identity: switching commands, targets or examples does not change what
  it is compiled with, and two applications of one directory never share one.

## Considered options

1. `AppId` moves to `flui-foundation`; the host takes `Option<&AppId>` at its start and the Linux
   windows carry it; the CLI writes the entry for every Linux build, named by what the window
   carries, with `StartupWMClass`.
2. The host keeps a `&str` and its own check; only the windows change.
3. A Linux launch configuration type in `flui-platform` with its own identifier field.

## Decision

Chosen option: 1, because it is the only one with a single checked identity and no second parser.

1. **The identifier is `flui_foundation::AppId`.** Its grammar (reverse DNS, labels of ASCII
   letters, digits and `_` led by a letter, no Java word, no Windows device name first) is a D-Bus
   well-known name and a desktop-file ID. Its longest form, `APP_ID_MAX_LEN` = 231 bytes, is set
   by the tightest name it is part of: the Windows single-instance pipe
   `\\.\pipe\flui-<id>-<session>` (256 characters, a session of up to ten digits), which leaves
   the mutex, the D-Bus name, the data directory and `<id>.desktop` room as well. A longer id is
   `AppIdError::TooLong` at `AppId::try_new`. `flui-app` exports it (`flui::app::AppId`,
   `AppIdError`); `flui-log`'s `IdentityError` keeps only the display name's refusals.
2. **A host is started with the id.** `CurrentHost::start_with(executor, Option<&AppId>)` and
   `CurrentHost::start_single(executor, &AppId)` take the parsed id; the single-instance name is
   the id itself, with no second check. `flui-app` passes `AppIdentity::app_id`.
3. **A Linux window carries it.** The host decides its `AppClass` once at start
   (`window::linux::class`): the id, or without one the executable's stem in ASCII
   (`flui_foundation::executable_class`: other characters as `_`, an X11 `STRING` being Latin-1).
   A Wayland window calls `xdg_toplevel.set_app_id` before its first commit; an X11 window writes
   `WM_CLASS` with the class as both instance and class, `_GTK_APPLICATION_ID` when it is an id,
   and `WM_HINTS` with `InputHint` set and input `True`.
4. **An identity is compiled in only when the project declares it.** `FLUI_APP_ID` reaches a
   build only from the project's cargo configuration, which every `flui` command (`flui run`,
   `flui build <any target>`) writes from the `[app] id` of `flui.toml`; no command passes an id of
   its own, and an id derived from the directory (`dev.flui.<dir>`, which names an unconfigured
   Apple bundle) is never compiled in. So one application has one identity whichever command or
   target builds it, and two examples of one directory do not share a data directory, a class or a
   single-instance name.
5. **The entry is named by what the window carries.** `flui build linux` writes
   `share/applications/<class>.desktop` for every build, with `StartupWMClass=<class>` and
   `MimeType` only when the application opens something: `<class>` is the declared id, else the
   executable's stem as `executable_class` writes it, so two examples keep two entries.

### Consequences

- Good, because the dock shows the entry's name and icon for the window and a launch's activation
  token is tied to the right application.
- Good, because an invalid id cannot reach a host: the host's own grammar and its warning path are
  gone.
- Bad, because an application without a declared id is matched by its executable's name only, and
  gets no data directory and no single instance until `flui.toml` declares one.
- Neutral, because the other hosts take the application's identity from their package and ignore
  the id; a Windows AppUserModelID is not set (a process id that no shortcut carries would break
  pinning), which `platform-parity.toml` records.

### Confirmation

The compiler: `start_with` and `start_single` take `AppId`, which only `try_new` makes. Tests:
`an_app_id_is_a_reverse_dns_name_every_platform_accepts` and `an_executable_is_classed_in_ascii`
(flui-foundation), `a_window_is_classed_by_the_application_id_or_the_executable` (flui-native,
pure), `a_window_carries_the_application_id_it_was_started_with` (flui-native's `x11_window` on
Xvfb with openbox, and `wayland_window::optional` under a headless sway, which lists toplevels to
another client), `the_entry_is_named_by_what_the_window_carries`,
`a_build_compiles_no_application_id_in_by_itself`,
`the_cli_and_the_framework_class_an_executable_alike` and
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
