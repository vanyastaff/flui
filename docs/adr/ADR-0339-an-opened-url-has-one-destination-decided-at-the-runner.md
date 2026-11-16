# ADR-0339: An opened URL has one destination, decided at the application runner

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0330](ADR-0330-navigation-is-a-typed-router-reached-from-the-context.md) §5
  in part (the paragraph on how the application runner routes what the platform opens: URLs held
  for a presentation that does not exist yet, and their refusal when the run ends);
  [ADR-0254](ADR-0254-window-contract.md) §4 in part (that the first instance hears a later
  launch after `reopen` whenever it shows no window)
- **Related:** [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md) (the declarations
  that admit a URL), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §4 (a
  handler's panic ends the run), [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (each
  URL opens under `FirstPanic`)

## Context and problem statement

ADR-0330 §5 decided how a URL reaches a Router: admitted by the application's declarations, opened
as a `Go`, or as the first Router's initial location before the first frame. For the runner it said
only that URLs whose presentation does not exist yet are held for it, bounded, and refused in the
log when the run ends. It did not decide what happens when that presentation never comes on its
own: a resident application (`Application::run_resident`), an `ExitPolicy::ExplicitQuit` application
after its last window closed, macOS after the last window closed. There a link the user followed
waited in silence until some window happened to open, which could be never. Only the Windows and
Linux single-instance hosts reopened a window first, and only for a later launch. The held URLs
were one list for one target, which a URL for another target replaced; a URL for a closed window
waited until the run ended; and URLs opened into a window whose installation then failed were lost
with its runtime.

How does an opened URL reach the application on every kind of host, with a bounded wait and a
reported end on every path?

## Decision drivers

- Accepted work is delivered or refused with a reason on every exit (`AGENTS.md`, "Recurring
  defects"): no URL waits without bound, none disappears unseen.
- One rule at the seam, holding on every host and the headless one (`AGENTS.md`, "Before each
  push", item 7), instead of a pre-step each host performs.
- A URL is untrusted input from another process: it must not be able to make a window open unless
  the application declares it.
- One way to navigate (ADR-0330): a deep link is navigation, not a second channel.
- The application states which window a link with no window opens in, without new API when the
  existing shape already says it.

## Considered options

1. The runner routes every opened URL to one typed destination; a URL for the application with no
   window opens the main window through the reopen route.
2. An application callback (`Application::on_open_url`) hears each URL and returns where it goes.
3. Hold the URL until some window opens (ADR-0330 §5 as built).
4. Each host reopens a window before it delivers an opened request.

## Decision

Chosen option: 1, because it is the only option that gives every URL a bounded end on every host,
keeps a hostile URL from opening a window, and adds no second navigation path.

```rust
// flui-view: the one admission rule, which the runner and `UiRuntime::open_url` both apply
impl OpenDeclarations {
    pub fn admit(&self, url: &OpenedUrl, page_origin: Option<&OpenedUrl>) -> Result<(), OpenUrlRefused>;
}

// flui-app (crate-private): where a URL for a target goes now
enum OpenDestination { Presentation(PresentationAddress), Pending, Closed, NotOpen }

// flui-app (crate-private): what waits, by the target's rule
struct HeldUrls {
    application: Option<String>,                    // the newest alone
    windows: Vec<(OpenTarget, VecDeque<String>)>,   // per window, in order, at most MAX_HELD_PER_WINDOW
}
```

- **§1. Admission first.** The runner admits each URL by `OpenDeclarations::admit`, the rule the UI
  runtime applies again, before it routes it. A URL the application does not open is refused as it
  arrives: it is never held and never opens a window.
- **§2. One destination.** A URL for a window goes to that window's presentation; one for the
  application to the presentation whose window was focused last, else the first opened. URLs held
  for that target open first, so the order is the platform's. A URL for a window whose presentation
  was removed is refused (`Closed`); one for a window with no presentation and no installation
  under way (nobody announced it, or its installation failed) is refused (`NotOpen`).
- **§3. The hold is bounded by its target's rule.** For the application as a whole only the newest
  URL is held: the window it opens in builds its first Router on one location, so a newer URL
  supersedes the held one, which is refused as superseded. For a window being installed, URLs are
  held in order, at most `MAX_HELD_PER_WINDOW` per window, later ones refused; a window's URLs never
  take another's place, and they wait only while its installation runs.
- **§4. A URL for the application with no window opens the main window** on hosts that have one: the
  runner takes the reopen route a Dock click takes, which shows the main window that `run` or
  `run_resident`'s factory builds, and that window's first Router opens on the URL. A reopen asked
  while a quit is asked, consulted or deferred waits for the quit: it is shown if the quit is
  abandoned and dropped if it commits. iOS, Android and the web route no reopen: an iOS URL
  addresses its scene, whose installation is under way when the URL is reported; an Android URL
  waits for the activity's window the host opens; the web's page is its window, and its address is
  the cold link the first window opens on.
- **§5. URLs lent to an installation come back if it fails.** URLs opened into a window's UI runtime
  before its root mounts are lent to the installation; it commits them, or a failed installation
  gives them back to the hold, where the application's waits for the next window and a window's own
  is refused.
- **§6. Every other end is reported.** What is held is refused in the log, by reason and never with
  the URL, when the bootstrap fails or panics (with the launch's files, which the application then
  never hears) and when the run ends. A panic in `on_open_files` or in a URL's navigation ends the
  run (ADR-0258 §4), which refuses what is held.
- **§7. A later launch of a single-instance application.** A launch that opens nothing is the user
  starting the application again: once answered, it asks the application to reopen while no window
  shows, as relaunching it does on macOS. A launch that opens URLs or files asks no reopen: the
  runner decides, and opens a window only for a URL it admits.

### Consequences

- Good, because a link with no window shows the application's main window on its page on every
  desktop host and the headless one, and a link it does not declare shows nothing.
- Good, because every held URL has a bounded wait and a reported end.
- Good, because deep links stay one mechanism: the Router.
- Bad, because a resident application that wants a link to open a window other than its main one
  has no hook; its main window's factory decides what that window shows.
- Bad, because the refusal is a log line: the host that delivered the URL hears nothing, and a later
  launch is acknowledged to its process before the runner refuses its URL
  ([gaps](../gaps.md#a-later-launch-is-acknowledged-before-its-link-is-routed)).
- Neutral, because a URL is parsed twice, by the runner and by the UI runtime, under one rule.

### Confirmation

`flui-app`'s `runner_open` table (`crates/flui-app/tests/runner_open.rs`):
`a_link_with_no_window_reopens_the_main_window_on_it` and
`a_link_that_launched_a_resident_app_opens_its_main_window_on_it` (§4),
`a_link_no_declaration_admits_opens_no_window` (§1),
`a_burst_of_links_with_no_window_opens_one_window_on_the_last` (§3: nine links near the length
bound, the window opens on the ninth),
`a_link_for_a_window_nobody_opens_is_refused_and_leaves_the_applications_link` and
`a_link_for_a_closed_window_is_refused_as_it_arrives` (§2),
`a_link_outlives_a_window_that_failed_to_install` (§5),
`a_link_after_the_quit_opens_no_window_and_is_refused_at_the_end`,
`a_reopen_during_a_deferred_quit_opens_when_the_quit_is_abandoned` and
`a_reopen_during_a_deferred_quit_is_dropped_when_the_quit_goes_ahead` (§4),
`a_panic_in_on_open_files_ends_the_run_and_refuses_the_link` (§6); the failed-bootstrap rows of
`app_handler_routes` (§6); in `flui-native`, `only_a_launch_that_opens_nothing_asks_for_a_reopen`
(§7); in `flui-widgets`, `declarations_admit_by_scheme_and_by_domain` and
`a_page_opens_the_urls_of_its_own_origin` (§1).

## Pros and cons of the options

### 1. One destination decided at the runner, the main window reopened

- Good, because the rule holds on every host, since every host reports through `AppHandler::opened`.
- Good, because admission before holding keeps another process from opening a window with a URL.
- Good, because the main window's factory already states what a reopened window shows.
- Bad, because a desktop application cannot choose another window for a link with no window.

### 2. An application callback decides

- Good, because a document application could open a new window per link.
- Bad, because it is a second way to navigate beside the Router (ADR-0330), and the next widget
  copies whichever it meets first.
- Bad, because a cold link would call the application before its bootstrap ran, or the runner would
  have to hold it anyway, so the callback adds a path without removing the hold.

### 3. Hold until a window opens

- Good, because it needs no window policy.
- Bad, because a link with no window waits without bound, the defect this record removes.

### 4. Each host reopens first

- Good, because the host knows whether a window is shown.
- Bad, because it is a policy each host must repeat; macOS's `application:openURLs:` and the
  headless host did not, and a host reopening before admission opens a window for a URL the
  application does not declare.

## More information

AppKit reports a relaunch as `applicationShouldHandleReopen:hasVisibleWindows:` and a URL as
`application:openURLs:`, with no window rule for the latter; Electron leaves `open-url` with no
window to the application; Tauri's deep-link plugin hands URLs to the application. Flutter has one
view and no multi-window routing, so the no-window case does not arise there.
