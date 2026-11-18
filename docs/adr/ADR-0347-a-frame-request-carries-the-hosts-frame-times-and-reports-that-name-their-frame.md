# ADR-0347: A frame request carries the host's frame times and reports that name their frame

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0257](ADR-0257-frame-timing-and-latency.md) whole (its decisions are restated
  here under the same numbers; decision 6 now names the frame a report describes, decision 8 the
  present's number), and with it what ADR-0257 superseded:
  [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) decision 8 only (the frame tick may be
  the frame's vsync instant) and [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md)
  decision 0 only (the number of frames in flight is a setting whose default is 2)
- **Superseded-by:** [ADR-0356](ADR-0356-a-surface-its-compositor-paces-presents-without-waiting-on-the-compositor.md)
  decision 5 only: a surface its compositor paces (Wayland) presents in `Mailbox`, else `Immediate`.
- **Superseded-by:** [ADR-0368](ADR-0368-a-frame-polls-its-tasks-runs-its-pipeline-then-its-post-frame-callbacks.md) decision 3 only: no idle work is bounded, and the request's times stay data.
- **Related:** [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §4 (a frame handed to a sink is stamped with its `FrameStamp`, which its settlement names) and §5
  (the latency setting is the window slot's), [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md)
  §2 (the event timeline), [ADR-0254](ADR-0254-window-contract.md) §5
  (`SurfaceTarget::pre_present_notify`), [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md)
  (a loop that blocks until woken), [ADR-0342](ADR-0342-a-value-added-to-a-closed-union-breaks-the-agent-protocol.md)
  (the protocol's version rule)
- **Sources:** Flutter 3.50.0, `flutter/flutter@ab2912035e engine/src/flutter/lib/ui/platform_dispatcher.dart:2309`
  (`FrameTiming.frameNumber`); Rune's `Present::Landed { seq, at }` (`anomalyco/rune`, branch `dev`,
  its frame module's present-time file); Microsoft's `IDXGISwapChain::GetLastPresentCount` and
  `DXGI_FRAME_STATISTICS` reference (a present's number, which `PresentCount` names); the Wayland
  `presentation-time` protocol (`wp_presentation.feedback` binds to the next commit,
  `presented`/`discarded`); Apple's `MTLDrawable.addPresentedHandler` reference (per drawable).

## Context and problem statement

A host wake reached the frame pump as a bare callback, so event-loop latency became animation and
scroll jitter at 120 and 144 Hz, though the platforms report the frame's vsync and, some, when the
frame will be shown. The engine configured every surface with two frames in flight in one literal,
so an application could not trade throughput for latency. ADR-0257 decided a frame request value, a
typed latency setting and presentation feedback on the request.

Its feedback named no frame. `FrameRequest::presented` was "the newest earlier frame the platform
reported shown". With two or three presents in flight, or a present the display skipped, the frame
shown is rarely the newest one presented, so a report could only be matched to "the current one":
the recurring defect of an answer matched to the current request instead of its ticket. The
platforms name the present they report (DXGI's present number, a Wayland commit's feedback object,
a Metal drawable's handler, Chromium's frame token, Flutter's `FrameTiming.frameNumber`); FLUI
dropped that name at the host. A report's refresh was a raw `Duration`, so a period of a nanosecond
or an hour could be reported while the checked `RefreshPeriod` existed. And ADR-0257 had gathered
partial supersessions and an amendment, so no one record stated the rule.

How does a host tell the frame pump what it knows about a frame, how does a report of a shown frame
name that frame, and how does an application choose how far the swapchain runs ahead?

## Decision drivers

- The frame path is synchronous and the pump reads its clock once per frame (ADR-0083).
- A platform that reports nothing behaves as one that reports: the host's own reading stands in.
- The tick never runs backwards, and a wrong platform timestamp neither freezes nor skips it.
- An answer is matched to its request by a ticket: a report of a frame that was superseded,
  skipped, shown late, drawn for a surface since replaced, or of a presentation the window left,
  names that frame and is refused where it does not belong.
- One identity per frame: the `FrameStamp` the raster side already settles a frame by.
- A host that cannot tell which present was shown reports nothing rather than guess.
- The loop blocks until woken (ADR-0249), so a present mode that never blocks would spin it.

## Considered options

1. A `FrameRequest` value and a typed `FrameLatency`; a report names its frame by the
   `FrameStamp` the present carried to the host.
2. As option 1, with a `PresentId` the host mints per present and returns, which the engine carries
   back to the runtime with the frame's settlement.
3. As option 1, with reports left unnamed and ordered by a report sequence number.
4. A bare callback and a setter per datum.
5. As option 1, with `Mailbox` as a `FrameLatency` variant.

## Decision

Option 1.

1. **A frame request is a value.** `on_request_frame` receives a `#[non_exhaustive]`
   `FrameRequest { vsync: EventTime, target_present: Option<EventTime>, presented:
   Option<FramePresented> }` on the event timeline of ADR-0207 §2. A backend fills what its
   platform reports and nothing it would have to invent: `vsync` is the platform's frame timestamp
   rebased onto the event epoch, or the backend's own reading at admission where the platform
   reports none; `target_present` is `None` unless the platform names when this frame will be
   shown. Neither is a prediction. A bare vsync converts into a request that knows nothing else.
2. **The pump takes the frame's time from the request.** The tick is the vsync instant, held
   between the previous frame's time and the host's own reading when it pumped: never earlier than
   the last frame, never later than the wake. A pump with no delivered request, or one whose vsync
   lies off the clock's range, uses the host's reading. A request a pump did not render is
   forgotten when the pump ends. The tick is a sampled instant, never the presentation time.
3. **`target_present` bounds idle work.** When a request names it, idle-priority tasks run only
   until that instant and the host's own deadline is not asked. A frame whose request names none
   bounds its idle work by the next refresh on the grid the presentation's newest report starts
   (decision 6), while that report is at most eight refreshes old: the first refresh after the
   host's own reading when the frame starts, and after the report. A live report trails the frame
   that reads it by the frames in flight (three to four refreshes, measured on Windows); an older
   one belongs to a window that was idle. Otherwise the deadline is the host's. The bound travels
   with the vsync in `HostInput::Frame`, so a recorded session replays it.
4. **Frame latency is a typed setting.** `FrameLatency` is `#[non_exhaustive]`: `Balanced` is two
   frames in flight, the measured default, and `Low` is one. `AppConfig::frame_latency` sets the
   application's; it is the window slot's, given when the slot is attached and changed by a
   `Configure` command, which is the per-window override, and the slot supplies it at every
   `Surface::configure`, the recovery and recreation paths included (ADR-0305 §5). A backend that
   ignores the bound (wgpu on GL) keeps ignoring it.
5. **`Fifo` stays the only present mode.** `Mailbox` never blocks `present()`, so the wake-driven
   loop would spin. It enters with a pacer that does not rely on the present call blocking, under
   its own ADR.
6. **A request carries presentation feedback, and a report names its frame.**

   ```rust
   pub struct FramePresented {
       pub frame: FrameStamp,              // the stamp the frame's present carried
       pub at: EventTime,                  // when the display started showing it
       pub refresh: Option<RefreshPeriod>, // the display's period then, where it is a cadence
   }
   ```

   `FrameRequest::presented` is the platform's newest report, since the window's last request, of
   an earlier frame shown, naming it. It is a report, never a prediction; a time named in advance
   stays `target_present`. A present that shows no frame of the window (a scene lent outside its
   frames) is never reported. Every host's reports pass one seam in the core host
   (`PresentationReports`):
   - each present tells it the presentation and surface generation of its frame (`presenting`); a
     report of a frame of another presentation or another surface generation is refused, and
     moving to another one forgets what was kept, so a late report of the old one is refused by
     its frame's identity, not by the order reports arrive in;
   - a report of a frame no newer than one kept or carried, or shown before it, is dropped, so
     neither the frames nor the times a window's requests carry run backwards;
   - a stamp ahead of the event clock's reading when the report arrives by at most `REPORT_LEAD` is
     the frame's time as the platform measured it, and the report is carried with that stamp; a
     stamp further ahead is no time of a frame, and the report is dropped;
   - the next request carries the newest report that arrived before it.

   The runtime drops a report whose frame's address is not the presentation the request is
   delivered to, before the frame clock or a recording sees it. `HostInput::Frame` carries the
   report as a `ShownFrame` (the frame's epoch and surface generation, the time, the period): a row
   is addressed to its presentation, which supplies the address when the input is delivered. The
   wire form carries the optional `frame` (`ShownFrameId { epoch, surface }`) from protocol 0.17, an
   additive version; a report recorded without its frame reads as no report.
7. **Whether a window reports is a capability,** `WindowCapabilities::reports_presentation`, with
   its reason kept in it (`Absent` with the `CapabilityAbsence`); no verb answers for it.
8. **The present carries its frame to the host, which carries it to the platform's answer.** The
   raster executor hands `RasterBackend::render` the frame's stamp, and the window slot hands it
   to `SurfaceTarget::pre_present_notify(frame: Option<FrameStamp>, statistics:
   SwapChainStatistics)` immediately before the present (the same stamp again when a suboptimal
   present is repeated). Where only the presenter can read the platform's record, it hands that
   over too.
   - **Win32 (DXGI)**: the presenter reads `GetLastPresentCount` with the frame statistics
     (`SwapChainStatistics::Dxgi(DxgiStatistics { last_present_count, shown: DxgiShown })`, where
     `DxgiShown` is a shown present, a break in the statistics, or nothing). The host keeps each
     present's frame by its number (`last_present_count + 1`), at most DXGI's largest buffer count,
     and reports a shown present as the frame kept for its `PresentCount`. A present number is a
     swap chain's, not a surface's identity, so frames are kept per surface generation: a present
     of another generation, or of no frame, forgets them; within one generation a present numbered
     no higher than the newest kept replaces the kept ones from that number on.
   - **Wayland**: each present of a frame asks `wp_presentation.feedback` with a request number
     whose frame the window keeps. A feedback and a frame callback asked before a present bind to
     the surface's next commit, whichever present makes it, so a feedback counts as committed once
     a frame callback armed with its present or a later one fired; the next present forgets every
     earlier feedback not confirmed, whose answer then names no frame, and a present of another
     surface generation forgets all. `discarded` reports nothing.
   - **AppKit (Metal)**: `nextDrawable` arms a frame slot in the drawable's presented handler, and
     `pre_present_notify`, on the main thread where the frames render, writes the frame into the
     slot of the drawable handed out last; a drawable no present of a frame claimed reports
     nothing.
   - **Headless**: each present is kept, per surface generation, until the simulated window system
     shows it (`simulate_presented`) or discards it (`simulate_discarded`), oldest first.
   - UIKit, Android, X11 and the web report no presentation (`reports_presentation` is `Absent`)
     and ignore the frame.

### Consequences

- Good, because a report can be matched to the frame whose input it measured, on every host, with
  no map: the stamp is the frame's own.
- Good, because a host with no timestamp keeps working: its vsync is its admission time.
- Good, because an application chooses latency without editing the engine.
- Good, because a period that is no cadence cannot be reported.
- Bad, because `Low` can halve the frame rate of a frame whose work exceeds the display period; it
  is opt-in for that reason.
- Bad, because the vsync clamp hides a backend whose rebase is wrong instead of failing.
- Bad, because `RasterBackend::render` and `SurfaceTarget::pre_present_notify` take the frame, and
  every implementation changes.
- Neutral, because the frame clock plans on the newest report's time; a consumer of the frame's
  identity (input-to-photon telemetry) is not built ([gaps](../gaps.md)).

### Confirmation

- `a_delivered_vsync_is_the_frames_time_held_between_the_last_frame_and_the_wake` and
  `a_target_present_time_bounds_the_idle_work_of_its_frame` (flui-runtime) pin decisions 2 and 3;
  `a_frame_latency_setting_reaches_the_configuration_and_survives_release` (flui-engine) decision 4.
- `a_presentation_report_bounds_the_idle_work_of_a_later_frame` (flui-runtime) pins the report's
  grid, the stale report and the refusal of another presentation's frame;
  `the_idle_work_of_a_frame_reached_late_ends_at_a_refresh_still_ahead` and
  `the_next_refresh_lies_on_the_grid_the_report_starts` (flui-scheduler) the grid.
- The seam: `a_presentation_report_rides_the_next_request`,
  `a_report_of_a_presentation_the_window_left_is_dropped`, `a_report_of_a_replaced_surface_is_dropped`,
  `a_report_stamped_far_ahead_does_not_outrank_the_reports_after_it` and
  `a_stream_of_stamps_that_lead_the_clock_reaches_every_request` (flui-native).
- The hosts: `a_shown_present_reports_the_frame_its_present_carried` and
  `a_replaced_surface_reports_its_own_frame` (DXGI), `a_feedback_whose_present_did_not_commit_reports_nothing`
  and `a_new_surface_forgets_the_old_surfaces_feedbacks` (Wayland),
  `a_presentation_report_names_the_present_it_describes` (headless, `tests/`), and
  `each_frame_reaches_the_backend_with_its_own_stamp` (flui-engine).
- The window conformance row `presentation_is_reported_exactly_when_claimed` checks that a report
  names a frame the fixture presented, and `a_window_whose_reports_name_another_frame_is_caught`
  that it fails when one does not; it has run on Win32 with DXGI statistics and on Weston with
  `wp_presentation`. `a_presentation_and_the_input_before_it_are_on_one_timeline` (flui-testing)
  pins the one timeline and the frame a recording names.

## Pros and cons of the options

### 1. The present carries the frame's stamp

- Good, because the identity is the one the raster side already settles a frame by, and its
  address and surface generation refuse a report of another presentation or a replaced surface.
- Good, because a host needs no mint and the runtime no map.
- Bad, because the platform contract names a foundation type the runtime mints.

### 2. A host-minted `PresentId`

- Good, because the platform contract owns its identity.
- Bad, because one frame gets two identities and the runtime maps them through the settlement,
  and every host must keep its mint across surface recreation.

### 3. A report sequence number

- Bad, because it orders reports but names no frame.

### 4. A bare callback and a setter per datum

- Bad, because the data would travel beside the callback, with an ordering a backend and the
  recording must keep right.

### 5. `Mailbox` as a latency variant

- Bad, because `present()` never blocks under it: the loop spins until a separate pacer exists.

## More information

A backend's platform timestamp is confirmed by a run on that platform; `docs/platform-parity.md`
names it. Rune records each present as `Present::Landed { seq, at }` from the drawable's handler;
Flutter's `FrameTiming.frameNumber` names the frame a timing measures.
