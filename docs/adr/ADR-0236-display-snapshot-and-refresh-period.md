# ADR-0236: The host owns an immutable display snapshot; the presentation owns its refresh period

- **Status:** Accepted
- **Date:** 2026-10-17
- **Superseded-by:** [ADR-0244](ADR-0244-os-version-floors-and-gates.md), decision 9's version and
  capability check style only; the Android refresh-rate hint it describes remains in force.
- **Superseded-by:** [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md), decision 7's Android
  idle poll only: the Android loop blocks until woken.
- **Superseded-by:** [ADR-0254](ADR-0254-window-contract.md), decision 5's delivery shape only: the
  window's display is a field of the window state snapshot and "your display may have changed"
  arrives through the window handler's state-changed obligation, not through `display_id()` and a
  callback. The snapshot, its keys and the refresh period stand.
- **Amends:** [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) decision 2 only: where
  the fallback period comes from. The fallback deadline, its 0.95 factor and the rest of the
  decision are unchanged.
- **Feeds:** [ADR-0212](ADR-0212-system-preferences-for-input.md) decision 4: the presentation's
  refresh interval that the haptic frame deadline multiplies is the value defined here.
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md) (delivery rules this follows),
  [ADR-0222](ADR-0222-window-appearance-is-a-presentation-observation.md) (the per-window
  observation shape), [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md).
- **Spec:** [display-pacing](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/plans/specs/display-pacing/design.md)

## Context

A display's refresh period reached the framework by one path only: the desktop runner read
`PlatformWindow::refresh_period` once at window installation and again in the resize handler,
and only when the window answered. A mode switch that leaves the window size alone (60 to 120 Hz,
an adaptive panel changing rate) was never seen, a move to a display that reports nothing kept the
old value, and the scheduler's jank statistics and the interaction metrics assumed 60 Hz. Windows
and Android reported no period at all; Windows answered the primary display for every window;
Android hard-coded its size and density; `DisplayId` was a public number each backend chose
(an `HMONITOR` on Windows, index 0 on four backends); winit enumerated monitors once at first
resume. No backend delivered a display change.

## Decision

1. **A snapshot, not live objects.** The host holds one immutable `DisplaySnapshot` of every display:
   id, name, bounds, usable bounds, scale factor and current refresh period. The live
   `PlatformDisplay` trait and `Platform::displays` are replaced by
   `Platform::display_snapshot`, sampled by the platform and committed by the host.
2. **The period is the current mode's, and may be unknown.** It is the rate of the mode the display
   is running, never its maximum; a variable-rate range is not modelled. A value that does not
   describe a cadence (zero, a zero denominator or zero timing total, a non-positive Wayland rate,
   non-finite, outside 1 ms to 1 s, Windows' 0 or 1 Hz "hardware default") is recorded as unknown
   (`None`); the validated `RefreshPeriod` type cannot hold any of them. The snapshot never
   substitutes 60 Hz.
   The consumer's fallback is 60 Hz (16.667 ms), a constant owned by `flui-runtime`: a too-short
   guess costs wakes on the most common displays, a too-long one costs at most one period of latency
   after idle. Flutter's `vsync_waiter_fallback.cc` and Chromium's `BeginFrameArgs::DefaultInterval`
   make the same choice. Where the Web has no native source, the period is an estimate from
   `requestAnimationFrame` timestamps and is documented as one.
3. **Identity is minted by the framework.** `DisplayId` is opaque and non-zero, issued per session
   by a registry keyed by the OS's native identity, never reused for another display, refusing
   permanently once the counter is exhausted and never wrapping. The same physical connector keeps
   its id across unplug and replug in the session; two identical monitors differ by connector;
   swapping cables swaps identities; an output with no stable native key gets a fresh id each time
   it appears. An id is meaningful only within one platform instance (one registry); ids from
   different instances or runs are not comparable. The keys are: the adapter LUID with the target id
   from `QueryDisplayConfig` on Windows (not `HMONITOR`, not the GDI device name; the monitor
   interface path is the fallback, and LUIDs change across reboots and GPU resets, which a session
   key tolerates); the display UUID on macOS; the `wl_output` name (version 4) on Wayland; the RandR
   output on X11 (a monitor is keyed by its first output); `Display.getDisplayId` on Android
   (`Display.getUniqueId` is not public SDK); the retained `UIScreen` object on iOS; the retained
   JS screen object on the Web. An object compared only while it is retained, never by address or
   label, so a freed address or a duplicate name cannot alias another display.
   A persistent cross-restart identity is not added here: it belongs with window placement restore,
   and `DisplayInfo` is `#[non_exhaustive]` so it can gain one without breaking the contract.
4. **Delivery follows ADR-0172.** Each native source registers its observers before sampling the
   seed. A change commits a new snapshot (its revision never wraps) before anyone is notified;
   invalidations from foreign threads wake the owner and never run application code there; a
   missing or panicking observer leaves the accepted snapshot and the delivery of the others intact.
   Accepted state and delivery debt stay separate: a window's last-delivered value is recorded only
   after its presentation applied it, so a failed apply leaves the debt and the next equal change is
   delivered. Shutdown closes admission before native observers are removed.
5. **The per-window observation is derived and carries no payload.** A window reports
   `display_id()` and a callback meaning "your display may have changed"; the host derives the
   window's `Option<WindowDisplay>` (display id and refresh period; the scale factor stays with the
   window's metrics observation) from the snapshot and notifies the presentation only when that value
   differs from the last delivered one. Pending updates coalesce as a whole value, so a newer
   "unknown" replaces a pending known period instead of losing to it. A change of another display, or
   of a field the window does not read, is silent. This is ADR-0222's shape. `PlatformWindow::display` and
   `PlatformWindow::refresh_period` are removed: the window and the snapshot had two sources for
   one fact.
6. **The runtime owns the presentation's refresh period.** The value delivered above is stored on
   the presentation. Every consumer reads that one value: the desktop fallback wake on the main and
   secondary windows (re-armed from the current period, not only on resize), the scheduler's
   stats-only frame budget (so jank is judged against the period of the window being produced), a
   interpolation window of frame-aligned pointer resampling, the profiler's jank line (through a
   field of the scheduler's frame span, since devtools cannot be a framework dependency), and the
   refresh interval of ADR-0212, whose
   factor times period is computed through `Duration::try_from_secs_f64` after checking that the
   factor is finite and positive (an unrepresentable product is "unknown"). The scheduler budget is
   set before the frame begins, and when one scheduler frame serves several presentations it uses
   the shortest period. A further consumer reads the same value; neither the platform nor the host
   changes.
7. **Each runner keeps its own native pacing mechanism and reads one period through one API.**
   The deadline-paced fallback wake is the desktop's, on the main and every secondary window.
   Android keeps its loop and replaces the fixed 16 ms idle poll with an interval derived from the
   period. iOS keeps its display link, configures its range from the screen's maximum and reports the
   interval the link actually received, published only when it leaves a hysteresis band. The Web
   keeps the browser's frame callback. A deadline-paced wake is not added where a platform frame
   source already paces: a second pacer would fight the first.
8. **The frame tick is not shifted to the presentation time, and carries no period.** A constant
   offset cancels in controllers (a run is anchored at its first tick), a change of offset at a 60 to
   120 Hz switch stalls or jumps an animation by about a frame through the clock's monotone hold, and
   no production code compares the tick with an absolute time. This can return with a consumer that
   starts from an absolute time (a fling from the pointer-up timestamp) and a defined clock domain.
   The period is read where it has a reader: pointer resampling (the interpolation window of the
   frame-aligned policy follows the period) and the refresh interval of ADR-0212.
9. **Version and capability checks are typed.** Each backend reads its OS version once at startup and
   keeps it typed in the platform instance. A capability check returns a typed handle rather than a
   bare number: an API-gated NDK function resolved through `dlsym` only when present (so a binary
   built for minSdk 23 still loads), an OS-version proof on Apple and Windows, a feature-detection
   proof on the Web (never a browser version). On Android API 30 and later the high refresh rate is
   requested while a presentation is active (running animation demand or an active pointer contact)
   and released after a short idle hold; below 30 the current rate is read only on configuration
   change and resume. The runtime has no platform edge (ADR-0083): it publishes the demand edge, a
   wake deadline ends the hold on a static screen where no frame runs, and the host app applies the
   hint to the window, again after the native window is replaced on resume. The request is a typed
   `FrameRateHint` (`Default` or `High`) passed to `PlatformWindow::set_frame_rate_hint`, whose
   default is a no-op: a backend without the capability needs no code, and the system may decline
   the request.
10. **One cache serves every native source.** `flui-native` keeps a `DisplaySource` per platform
    instance: the id registry, the last published snapshot and a stale flag that any thread may
    set. `display_snapshot` runs on every owner wake, so a backend with change notifications
    invalidates the source and reads the OS again only when it is stale; the flag is cleared before
    the read, so an invalidation that lands during a sample survives it. `snapshot_with_aux`
    publishes a sample that also yields data a reader resolves against the snapshot (Windows'
    monitor handles) and commits that data only after the snapshot is published, so a reader never
    sees data for displays the snapshot lacks. A window on a display the snapshot lacks (a monitor
    plugged in since) answers no display, never the primary, and `note_miss` invalidates once per
    snapshot revision however often the miss repeats; `note_miss_with` additionally takes the call
    that wakes the owner, for a backend whose owner would otherwise sleep until something else wakes
    it (Android). An unchanged sample answers the previous snapshot with its revision.

## Consequences

- An embedder implementing `Platform` returns a snapshot and per-window ids; the removed methods
  have no replacement beyond the snapshot. This is a pre-1.0 break, documented in the changelog.
- Native sources are per OS rather than per window backend: the winit backend no longer enumerates
  monitors itself and gets the snapshot from its OS's source. On Linux this is a new display module
  over `wl_output` and `zxdg_output_v1` or RandR, whose parsing is a pure model, designed as the first
  piece of a native Linux backend. The source reads the compositor on a connection of its own and a
  thread wakes the owner on change, so the winit backend takes its snapshot from it and winit's own
  monitor list is read nowhere.
- The Android change source is not a `DisplayManager` listener (it needs a Java class that a
  NativeActivity application does not have): refresh-rate changes come from the NDK choreographer
  callback on API 30 and later, everything else from configuration changes, resume and the existing
  sampling timer.
- The winit backend matches a window to its display by the native output identity (the RandR CRTC,
  `wl_output` global name), never by bounds or label; mirrored outputs stay distinct. On Wayland a
  move between two outputs of equal scale that winit does not report stays unseen and is documented.
- The first change keeps every existing backend feeding the snapshot with what it already reads
  (macOS's current display mode, winit's monitor rate), so no panel regresses to 60 Hz while the
  per-OS sources land.
- ADR-0212's `unknown_refresh_period_uses_the_60_hz_fallback` row and per-display gesture values
  read the presentation's period and display id defined here.
- Tests: `display_identity_contract`, `display_delivery_contract`, `window_display_observation_contract`
  and `display_timing_contract` run on the headless backend and a pure Linux model. Reading real
  values and observing a live change runs natively; a platform is marked verified only after values
  printed and one live change observed. macOS and iOS are type-checked only on the development host.

## Alternatives rejected

- **Keep live `PlatformDisplay` objects and add events.** Every consumer queries the OS itself, from
  threads some platforms forbid, and no coherent view of all displays exists before the first window.
- **A payload on the window callback.** It can race a newer snapshot; the consumer must re-read the
  accepted state anyway.
- **Report the maximum rate.** It over-reports on adaptive panels running a lower mode.
- **Poll the window each frame.** It puts an OS query on the frame path and still misses changes
  while idle.
- **Persist a display identity across restarts now.** It has no consumer before window placement
  restore and would fix a shape that feature should decide.
