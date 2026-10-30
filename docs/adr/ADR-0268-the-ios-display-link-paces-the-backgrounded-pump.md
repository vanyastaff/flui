# ADR-0268: On iOS the display link paces the backgrounded pump, and the runner never sleeps on the main thread

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md), its clause "iOS keeps
  its sleep", which kept the iOS half of [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md)
  decision 5: the iOS runner's backgrounded pump no longer sleeps `BACKGROUNDED_PUMP_PACE`.
- **Related:** [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) (the platform paces
  production, not a sleep), [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) §4 (Android's
  `BackgroundPace` deadline)

## Context and problem statement

`flui-app`'s iOS frame driver handles a wake whose scheduler has frames disabled
(`WakeAction::PumpBackground`) by running `UiRuntime::pump_background` and then sleeping 16 ms
on the main thread. During the sleep, UIKit delivers no touches, scene events or display-link
callbacks. Android replaced the same sleep with a deadline, `BackgroundPace` (ADR-0249 §4).

The facts on iOS (`flui-native`'s `window::apple::ios`, `flui-app`'s `runner::ios`):

| Fact | Where |
|------|-------|
| The driver runs only on a frame request, which comes from a `CADisplayLink` tick | `runner/ios.rs` `route_frame_requested`; `FluiView::on_display_link` |
| The link is held paused on `Background` and `Detached`, the same edges that disable frames (`Hidden`, `Paused`, `Detached`) | `IOSWindow::set_frame_tick_paused`; `Scheduler::handle_app_lifecycle_state_change` |
| A tick comes only while a frame is owed (`FrameDemand`); a request made while the link is held waits in the demand and is served on release | `FluiView::set_display_link_paused`, `IOSRedraw` |
| The owner's wake deadline wakes the owner signal, not the frame driver | `owner/apple/ios.rs` `arm_deadline` |

The backgrounded arm is therefore reached only by a tick that lands between the scheduler disabling
frames and the native hold. A pump there that leaves work behind asks for a frame through the
demand, and the link fires at most once per refresh, or not at all once held. Should iOS keep the
sleep, take Android's deadline, or let the link pace the arm?

## Decision drivers

- The main thread never blocks on a timer: UIKit's input and lifecycle delivery run on it (ADR-0058).
- No accepted work is dropped: a pump the pace defers must still run.
- One frame-demand state machine per platform (`flui-native`'s `owner::frame_demand`).

## Considered options

1. **Keep the sleep.**
   - Good: no change.
   - Bad: it blocks UIKit for 16 ms on every backgrounded wake, and it adds nothing a display-link
     tick doesn't already bound.
2. **Android's `BackgroundPace` deadline.**
   - Good: the same shape on both mobile backends.
   - Bad: on iOS the deadline wakes the owner signal, which doesn't run the frame driver. A tick
     the pace turns away leaves no frame owed, so the link pauses, and the deferred pump never runs
     until something else asks for a frame. That drops accepted work.
3. **The display link paces the arm: no sleep.**
   - Good: the pump runs once per tick, a tick needs an owed frame, and the link delivers at most
     one per refresh and none while held.
   - Bad: in the transition race, a self-re-arming task can run one background pump per refresh
     until the hold lands, which is bounded by the display rate.

## Decision

Chosen option: 3, because the display link is already the pace on iOS. A deadline would need a
second wake path to the frame driver, and that path would only reproduce what the link does.

`IosFrameDriver::wake`'s `PumpBackground` arm runs `UiRuntime::pump_background` and returns.
`BACKGROUNDED_PUMP_PACE` and `BackgroundPace` are compiled for Android (and tests) only, so no other
runner can use the pace as a sleep.

### Consequences

- Good, because no FLUI runner sleeps on its loop or main thread any more.
- Good, because UIKit's touches and scene events are delivered during a backgrounded pump.
- Bad, because the pace now rests on the link's hold and frame demand, which run only on a device.
- Neutral, because Android keeps `BackgroundPace` (ADR-0249 §4) unchanged.

### Confirmation

- The compiler: `BACKGROUNDED_PUMP_PACE` is `#[cfg(any(target_os = "android", test))]`, so a sleep
  on it doesn't build for iOS. CI's `cross-typecheck (aarch64-apple-ios)` job builds that target.
- `a_frame_is_owed_only_until_a_frame_takes_it` (`flui-native`) covers the demand the link waits on.
- A run on an iPhone: a tick while backgrounding doesn't stall touch delivery
  and an idle backgrounded app runs no pump.
