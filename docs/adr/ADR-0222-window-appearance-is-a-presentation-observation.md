# ADR-0222: The light/dark theme is observed per window, as the user's choice

- **Status:** Accepted
- **Date:** 2026-10-17
- **Supersedes:** [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §1, the
  `window_appearance` entry of `OwnerPlatform`'s operation list only.
- **Superseded-by:** [ADR-0254](ADR-0254-window-contract.md) for the delivery shape only: the
  appearance is a field of the window state snapshot and its change arrives through the window
  handler's state-changed obligation, not through `appearance()` and `on_appearance_changed`. The
  observation itself (per window, the user's choice, owner-thread delivery) stands.
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md) (brightness is
  presentation-specific).

## Context

The theme reached the framework through two contracts: `Platform::window_appearance` (also
on `OwnerPlatform`), and `PlatformWindow::appearance` with `on_appearance_changed`. No
production path read the platform one, and most backends answered light there whatever the
theme. The window one was wired only for the desktop main window. On Windows it read back
the DWM dark title-bar flag that the backend had itself forced on, so every desktop reported
dark.

## Decision

1. `PlatformWindow::appearance` is the only source. It reports the user's light/dark theme
   as it applies to that window, never a value the backend chose for its own chrome. A
   backend that cannot observe the theme reports light.
2. `on_appearance_changed` fires on the owner thread only when the value `appearance()`
   returns has changed. Settings notifications that leave the theme alone stay silent.
3. `Platform::window_appearance` and `OwnerPlatform::window_appearance` are removed. A
   host-wide theme would compete with the per-window value that ADR-0172 makes
   authoritative.
4. Every runner registers the observer before it samples the seed, through one
   `flui-app` installation step. The desktop main and secondary windows, iOS, Android and
   the Web all use it, so `MediaQueryData::platform_brightness` starts at the user's theme
   in every window.

The native source on each backend is recorded in `crates/flui-platform/ARCHITECTURE.md`.

## Consequences

- An embedder that read `OwnerPlatform::window_appearance` reads the window's
  `appearance()` instead.
- `platform_brightness_follows_the_window_appearance` in `flui-app` pins rule 4 through
  the headless backend. On Windows, the native contract case
  `appearance_is_the_theme_the_chrome_follows_not_the_chrome_flag` pins rules 1 and 2.
