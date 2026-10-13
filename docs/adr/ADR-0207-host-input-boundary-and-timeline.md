# ADR-0207: A UI runtime takes input through one `HostInput` boundary on one timeline, on its owner thread

- **Status:** Accepted (2026-10-13); implemented by the interaction completeness spec
- **Date:** 2026-10-13
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (owner-affine runtimes, the
  runtime-local FIFO), [ADR-0082](ADR-0082-platform-api-contract-crate.md) (§4: callbacks lose
  `Send`), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md) (the vocabulary `HostInput` carries),
  [ADR-0159](ADR-0159-gesture-arena-owner-delivery.md),
  [ADR-0161](ADR-0161-immutable-owner-local-gesture-recognizers.md),
  [ADR-0163](ADR-0163-presentation-owned-pointer-resampling.md),
  [ADR-0199](ADR-0199-presentation-sampling-recovery.md); spec
  [docs/plans/specs/flui-interaction/completeness/](../plans/specs/flui-interaction/completeness/design.md);
  research [threading model](../research/2026-10-13-interaction-threading-model.md),
  [trace format](../research/2026-10-12-interaction-input-followups.md).

## Context

Platform input, window events, frames and assistive-technology requests reach a UI runtime by four
separate paths, so nothing observes everything that influences an arena decision (focus loss, a
scale change, a frame, an AT action). The arena and contact recognizers read `web_time::Instant`
for their deadlines while events carry `EventTime`, so a recording cannot replay to the same
decisions. Sequence identity is per subsystem: pointer ids, wheel latch state and the scroll
channel each key work differently, and a latch keyed by nothing but the device cannot tell two
windows apart. Pointer kinds are closed over mouse, touch, pen and trackpad, leaving gamepad
cursors and automation without a device of their own. GPUI drops a native event that arrives
while its window is being updated (threading research, "GPUI"). Every toolkit surveyed arbitrates
gestures on its UI thread; the ones that scroll off-thread make chaining declarative or answer late.

## Decision

1. **One boundary.** `HostInput` in `flui-interaction` is the only input a UI runtime accepts:

   ```rust
   #[non_exhaustive]
   pub enum HostInput {
       Platform(PlatformInput),
       Window(WindowEvent),
       Frame { vsync: EventTime },
       Accessibility(accesskit::ActionRequest),
   }
   impl GestureBinding { pub fn handle(&self, input: HostInput) -> DispatchOutcome; }
   ```

   `DispatchOutcome` is `#[must_use]`. There is no source trait: the set is closed. The
   `flui-app` runner folds its four paths into it; recording, replay and observers see exactly
   this value. `HostInput` and its payloads hold no `Rc`, callback or borrowed render object.
2. **One timeline.** Every deadline — arena resolution, long press, `RequireFailure`, wheel and
   phase latch idle timeouts, spring-loading, the web's synthesised pinch End — is an `EventTime`
   and is compared with the interaction clock, which reads the same epoch. Arena and recognizer
   code calls no `Instant::now()`. Natively the interaction clock is the backend's monotonic
   reading on the event epoch; headless, one virtual source advances the interaction clock to
   each entry's arrival time and the `FrameClock` to each `Frame`'s vsync.
3. **Replay determinism per runtime.** A runtime's `HostInput` stream, with `EventTime`, arrival
   time and frames, replays to identical arena decisions. Anything entering a runtime from
   another thread (worker results, cross-runtime DnD, close requests) is a recorded entry at its
   point of application. No order between runtimes is promised: Win32 queues input per thread.
4. **`InputSequenceId`.** `GestureBinding` mints one per contact at Down and per wheel, keyboard,
   AT and auto-scroll sequence. It is a newtype over `NonZeroU64` with a private field, `Copy +
   Eq + Ord + Hash`, no `Default` and no `From<u64>`; the mint is `IdMint::next() -> Result<_,
   IdExhausted>` over an owner-local `Cell<u64>`, never reuses a value, and refuses permanently
   once exhausted. Events, gesture details, the scroll channel (which mints none of its own), DnD
   sessions, haptic requests, recordings and observers carry it.
5. **`InputSurface`.** A window or an embedded surface is the routing and recording root. Pointer
   sequences, capture, hover, coalescing and latch are keyed by `(InputSurface, DeviceId)` from
   their first implementation, never by a process-wide "current pointer".
6. **`PointerKind::Virtual { device }`.** `PointerKind` in `flui-platform-api` is
   `#[non_exhaustive]` and gains a virtual pointer with its own `DeviceId` (gamepad cursor,
   automation, later XR) before the recording format is frozen. Hit testing stays a point query;
   a ray query arrives as a new hit-test request type, not as a vector field in recordings.
7. **Reentrant native events are queued.** A native event that arrives while a callback is
   running goes to the backend's queue and is delivered after the callback returns, in order
   (ADR-0027 §3's FIFO); it is never dropped.
8. **Threading.** Recognizers, the arena, the scroll channel, focus, the IME host, DnD sessions
   and widget haptics run on the owner thread of their `UiRuntime` and are `!Send`;
   `GestureBinding` joins the pinned owner types. A window's platform thread is its owner
   thread: concurrency comes from more owner threads, each creating its own windows (Win32
   only; winit allows one event loop per process, AppKit and UIKit one main thread). Scroll,
   fling and DirectManipulation inertia stay on the owner thread, because
   `NestedScrollConnection` returns consumed deltas synchronously from user code. The seam kept
   for a later compositor-side scroller: scroll steps, policies and `InputSequenceId` are `Send`
   data, built-in fling curves are `Send` types, and every offset change passes one dispatcher.
   Between runtimes DnD moves only data, Win32 pointer capture crosses threads only while a
   button is held, and haptics are issued on the owner thread of the window that received the
   input. `+ Send` leaves `PlatformWindow::on_input` once owner-thread registration (ADR-0082
   §4) lands.

## Consequences and verification

The arena, channel, latch, DnD, haptics, recording and observer share one identity, one clock
and one entry point. A blocked owner stalls only its runtime, as in Compose, UIKit and Android.

- `replaying_a_recording_twice_gives_identical_arena_decisions` and
  `deadlines_fire_on_event_time_without_wall_clock_reads` in `flui-interaction`'s tests.
- `input_sequence_ids_are_never_reused` and `an_exhausted_sequence_mint_refuses_permanently`,
  through a test-only mint started near the terminal value.
- `a_native_event_during_a_callback_is_queued_not_dropped` against the headless backend; the
  Win32 and AppKit nested-pump rows run on a native host only.
- `latch_and_hover_are_keyed_by_surface_and_device` with two surfaces sharing one `DeviceId`.
- Static assertions in `tests/`: `GestureBinding: !Send + !Sync`; `HostInput`, `InputSequenceId`,
  `InputSurface` and `ScrollStep` are `Send + 'static`. trybuild `compile_fail`
  `gesture_binding_stays_on_its_thread` in `crates/flui-interaction/tests/compile_fail/`, and
  `compile_pass` rows proving `Rc`-capturing recognizers and scroll connections are accepted.

Linux CI runs all rows through the headless backend. Per-thread Win32 runtimes and cross-thread
capture are unverified until measured on Windows.

## Alternatives rejected

- **Splitting the platform thread from the owner thread.** Every synchronous answer the OS needs
  (`default_prevented`, TSF pulls, DirectManipulation `SetContact`, web `preventDefault`) would
  become late; Flutter merged these threads on every platform (threading research, §3).
- **Compositor-side scrolling now.** It requires declarative-only chains; Chromium, Firefox APZ and
  WinUI InteractionTracker either fall back to the main thread or drop scroll-linked callbacks.
- **A cross-runtime total order for replay.** Not reproducible under per-thread input queues.
- **Separate sequence ids per subsystem.** A fling handed between channel members, a haptic tied
  to the gesture and a recording row would then need translation tables.
