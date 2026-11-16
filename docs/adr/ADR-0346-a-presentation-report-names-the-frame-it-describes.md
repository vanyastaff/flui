# ADR-0346: A presentation report names the frame it describes

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0257](ADR-0257-frame-timing-and-latency.md) decision 6's report shape
  (`FramePresented { at, refresh: Option<Duration> }`, "the newest earlier frame") and its first
  ordering rule, and decision 8's `SwapChainStatistics` shape (`Dxgi`, `DxgiDisjoint`). The rest
  of both decisions stands: a request carries the report, every host's reports pass one seam
  (`PresentationReports`), the lead rule, the capability.
- **Related:** [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §4 (a frame handed to a sink is stamped with its `FrameStamp`, which its settlement names),
  [ADR-0254](ADR-0254-window-contract.md) §5 (`SurfaceTarget::pre_present_notify`),
  [ADR-0342](ADR-0342-a-value-added-to-a-closed-union-breaks-the-agent-protocol.md) (the
  protocol's version rule)
- **Sources:** `flutter/flutter@3.50.0 engine/src/flutter/lib/ui/platform_dispatcher.dart:2309`
  (`FrameTiming.frameNumber`); Rune's `Present::Landed { seq, at }` (`anomalyco/rune`, branch
  `dev`, its frame module's present-time file); Microsoft's `IDXGISwapChain::GetLastPresentCount` and
  `DXGI_FRAME_STATISTICS` reference (a present's number, which `PresentCount` names); the Wayland
  `presentation-time` protocol (`wp_presentation.feedback` per commit, `presented`/`discarded`);
  Apple's `MTLDrawable.addPresentedHandler` reference (per drawable).

## Context and problem statement

`FrameRequest::presented` carried "the newest earlier frame the platform reported shown", with no
name for that frame. A swap chain holds two or three presents in flight and a display drops a
present it never latched, so the frame shown is rarely the newest one presented: a consumer that
measures input-to-photon latency, or matches a report to the frame whose input it carried, had
nothing to match on and could only take "the current one". The platforms all name the present
they report: DXGI by the present's number, Wayland by the feedback object of the commit, Metal by
the drawable's handler, Chromium by a frame token, Flutter by `FrameTiming.frameNumber`. FLUI
dropped that name at the host. `FramePresented::refresh` was a raw `Duration`, so a report could
carry a period of one nanosecond or an hour while the checked `RefreshPeriod` existed.

The frame already has an identity end to end: the runtime stamps each frame it hands a sink with a
`FrameStamp` (its presentation's address, its epoch, the surface generation it was drawn for), and
the raster side settles the frame by it (ADR-0305 §4). How does a report name its frame?

## Decision drivers

- An answer is matched to its request by a ticket, never by "the current one" (AGENTS.md "Before
  each push" item 4): a report of a frame that was superseded, skipped, shown late, or drawn for a
  surface since recreated names that frame.
- One identity per frame: a second mint beside `FrameStamp` would need a map between the two.
- A host that cannot tell which present was shown reports nothing rather than guess.
- The frame path stays synchronous and allocation-free; every in-flight table is bounded.

## Considered options

1. The raster side hands each present's `FrameStamp` to the surface target, and the host carries
   it to the platform's answer for that present.
2. The host mints a `PresentId` per present in `pre_present_notify` and returns it; the engine
   carries it back to the runtime with the frame's settlement, which keeps a map to its frames.
3. Keep the report unnamed and add a sequence number of reports.

## Decision

Option 1.

### §1 The report names its frame

```rust
pub struct FramePresented {
    pub frame: FrameStamp,               // the stamp the present carried
    pub at: EventTime,
    pub refresh: Option<RefreshPeriod>,  // a cadence, or none
}
```

`FrameRequest::presented` is the platform's newest report since the window's last request, naming
its frame. A present that shows no frame of the window (a scene lent outside its frames) is never
reported.

### §2 The present carries the stamp to the host

`SurfaceTarget::pre_present_notify(&self, frame: Option<FrameStamp>, statistics)`; the raster
executor hands `RasterBackend::render` the frame's stamp, and the window slot hands it on with the
present (the same stamp again when a suboptimal present is repeated). Each reporting host carries
it to the platform's answer for that present:

- **Win32 (DXGI)**: the presenter reads `GetLastPresentCount` with the frame statistics
  (`SwapChainStatistics::Dxgi(DxgiStatistics { last_present_count, shown: DxgiShown })`); the
  host keeps each present's frame by its number (`last_present_count + 1`), at most DXGI's largest
  buffer count, and reports a shown present as the frame kept for its `PresentCount`. A count that
  went back is a new swap chain: the frames kept are forgotten. A shown present no frame is kept
  for is not reported.
- **Wayland**: each present of a frame asks `wp_presentation.feedback` with the frame as the
  feedback's user data; `presented` reports it, `discarded` reports nothing.
- **AppKit (Metal)**: `nextDrawable` arms a frame slot in the drawable's presented handler;
  `pre_present_notify`, on the main thread where the frames render, writes the frame there; a
  drawable no present of a frame claimed reports nothing.
- **Headless**: each present is kept until the simulated window system shows it
  (`simulate_presented`) or discards it (`simulate_discarded`), oldest first.
- UIKit, Android, X11 and the web report no presentation (`reports_presentation` is `Absent`) and
  ignore the frame.

### §3 The seam orders reports by their frame

`PresentationReports` learns from each present which presentation the window presents
(`presenting`). It drops a report of another presentation's frame (an incarnation the window left)
and, within one presentation, a report of a frame no newer than one kept or carried, or shown
before it; the lead rule of ADR-0257 decision 6 stands. The runtime drops a report whose frame's
address is not the presentation it is delivered to, before the frame clock or a recording sees it.

### §4 A recording names the frame within its presentation

`HostInput::Frame` carries a `ShownFrame` (epoch, surface generation, time, refresh): a row is
addressed to its presentation, which supplies the address when the input is delivered. The wire
form gains the optional `frame` (`ShownFrameId { epoch, surface }`) in protocol 0.16, an additive
version: a reader on 0.15 ignores it, and a 0.15 recording's report, which names no frame, reads as
no report.

### Consequences

- Good, because a report can be matched to the frame whose input it measured, through every
  host, with no map: the stamp is the frame's own.
- Good, because a period that is no cadence cannot be reported.
- Bad, because `RasterBackend::render` and `SurfaceTarget::pre_present_notify` take one more
  argument, and every implementation changes.
- Neutral, because the frame clock still plans on the newest report's time; a consumer of frame
  identity (latency telemetry) is not built here.

### Confirmation

- `a_shown_present_reports_the_frame_its_present_carried` (flui-native, the DXGI model): two
  presents in flight, a skipped present, a present without a frame, a new swap chain.
- `a_presentation_report_names_the_present_it_describes` (flui-native `tests/`, headless): two
  presents in flight, the first is reported; a discarded present never is.
- `a_report_of_a_presentation_the_window_left_is_dropped` and
  `a_presentation_report_rides_the_next_request` (the seam).
- `each_frame_reaches_the_backend_with_its_own_stamp` (flui-engine).
- `a_presentation_report_bounds_the_idle_work_of_a_later_frame` (flui-runtime): a report naming
  another presentation's frame bounds nothing.
- The window conformance row `presentation_is_reported_exactly_when_claimed` checks that a
  report names a frame the fixture presented, and
  `a_window_whose_reports_name_another_frame_is_caught` that it fails when one does not.

## Pros and cons of the options

### 1. The present carries the frame's stamp

- Good, because the identity is the one the raster side already settles by.
- Good, because a host needs no mint and the runtime no map.
- Bad, because the platform contract names a foundation type the runtime mints.

### 2. A host-minted `PresentId`

- Good, because the platform contract owns its identity.
- Bad, because the runtime must map present ids to frames through the settlement, a second
  identity for one frame, and every host must mint across surface recreation.

### 3. A report sequence number

- Bad, because it orders reports but still names no frame.

## More information

Rune records each present as `Present::Landed { seq, at }` from the drawable's presented handler;
Flutter's `FrameTiming.frameNumber` names the frame a timing measures. FLUI's report names the
frame by the stamp its present carried on every host that reports.
