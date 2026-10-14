# ADR-0207: A UI runtime takes input through one `HostInput` boundary on one timeline, on its owner thread

- **Status:** Accepted (2026-10-13); implemented
  (`docs/plans/specs/flui-interaction/completeness/tasks.md`) except §8's last sentence:
  `PlatformWindow::on_input` keeps `+ Send` until every backend stores its callbacks owner-only
  (ADR-0082 §4 step 1 is done for Win32 only; the shared winit/headless storage is a `Mutex`). The
  Win32 and AppKit nested-pump rows of §7 are not written yet.
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
cursors and automation without a device of their own. In GPUI, an event that arrives while its
window is being updated finds the callback taken: it is not delivered, and AppKit is answered
`NO` (threading research, "GPUI"). Every toolkit surveyed arbitrates
gestures on its UI thread; the ones that scroll off-thread make chaining declarative or answer late.

## Decision

1. **One boundary.** `HostInput` is the only input a UI runtime accepts. It is defined in
   `flui-runtime`, because it is the runtime's boundary and `flui-interaction` must not depend on
   accesskit:

   ```rust
   #[non_exhaustive]
   pub enum HostInput {
       Platform(PlatformInput),
       Window(WindowEvent),
       Frame { vsync: EventTime },
       Accessibility(accesskit::ActionRequest),
   }
   ```

   The runtime admits it through its owner FIFO (`PresentationDispatcher::dispatch`; the frame
   driver's `FrameDispatcher::deliver` admits the same `Frame` work) and unwraps it: it calls
   `GestureBinding` with the interaction-owned parts only — pointer input
   (`dispatch_pointer_on`, which returns a `#[must_use]` `DispatchOutcome` naming the input's
   sequence), the window observation and the frame tick — and it passes `Accessibility` to the
   semantics owner (`flui-semantics` already depends on accesskit). The `Window` payload is the
   runtime's normalized `WindowObservation` (`flui_platform_api::WindowEvent` keys raw window
   events by native id). No `flui-interaction` signature names
   `HostInput`. That variant couples
   `HostInput` to accesskit's 0.x `ActionRequest`: an accesskit bump that changes it is a
   breaking change of this boundary and of the recording DTO. There is no source trait: the set
   is closed. The `flui-app` runner folds its four paths into it; runtime dispatch and replay consume
   exactly this value; a recording holds it as `InputRecord::Input` rows among its decision,
   preference, gap and end rows (ADR-0215 §1), and `GestureObserver` receives derived gesture
   events, never `HostInput`. `HostInput` and its payloads hold no `Rc`, callback or borrowed render
   object.
2. **One timeline.** Every deadline — arena resolution, long press, `RequireFailure`, wheel and
   phase latch idle timeouts, spring-loading, the web's synthesised pinch End — is an `EventTime`
   and is compared with the interaction clock, which reads the same epoch. Arena and recognizer
   code calls no `Instant::now()`. Natively the interaction clock is the backend's monotonic
   reading on the event epoch; headless, one virtual source advances the interaction clock to
   each entry's arrival time and the `FrameClock` to each `Frame`'s vsync.
3. **Replay determinism per runtime.** A runtime's recorded `HostInput` stream, with `EventTime`,
   arrival time and frames, and with the `SystemPreferences` revisions applied during it
   (recorded in order, ADR-0215 §1), replays to identical arena decisions against a fixed
   application-state timeline. Application state changed by sources other than input (worker
   results, timers in application code) is outside the recording: a worker result that rebuilds
   the hit-test tree between Down and Move is not replayable from `HostInput`. No order between
   runtimes is promised: Win32 queues input per thread.
4. **`InputSequenceId`.** The type lives in `flui-foundation` (beside `DataTransferId`) so contract crates such as `flui-platform-api` can name it; `GestureBinding` mints one per contact at Down and per wheel, keyboard,
   assistive, programmatic and auto-scroll sequence; the runtime requests assistive and
   programmatic ones from the binding rather than minting its own. A contactless source gets a
   `DeviceId` from the binding too: assistive sequences use the accessibility bridge's device id
   for their surface, programmatic ones a reserved programmatic device id per surface. It is a newtype over `NonZeroU64` with a private field, `Copy +
   Eq + Ord + Hash`, no `Default` and no `From<u64>`; the mint is `IdMint::next() -> Result<_,
   IdExhausted>` over an owner-local `Cell<u64>`, never reuses a value, and refuses permanently
   once exhausted. Events, gesture details, the scroll channel (which mints none of its own), DnD
   sessions, recordings and observers carry it; a haptic request carries it only as optional
   diagnostic context (`None` outside dispatch, ADR-0210), so programmatic haptics mint nothing.
5. **`InputSurface`.** A window or an embedded surface is the routing and recording root. Pointer
   sequences, capture, hover, coalescing and latch are keyed by `(InputSurface, DeviceId)` from
   their first implementation, never by a process-wide "current pointer".
6. **`PointerKind::Virtual`.** `PointerKind` in `flui-platform-api` is `#[non_exhaustive]` and
   gains a fieldless virtual kind (gamepad cursor, automation, later XR) before the recording
   format is frozen. Its device is the event's `DeviceId`: a field would be a second identity, and
   `PointerKindSet` cannot hold a data-carrying variant. Hit testing stays a point query;
   a ray query arrives as a new hit-test request type, not as a vector field in recordings.
7. **Reentrant native events are queued.** A native event that arrives while a callback is
   running goes to the backend's queue and is delivered after the callback returns, in order
   (ADR-0027 §3's FIFO); it is never dropped. An event that needs a synchronous answer (AppKit's
   `performKeyEquivalent:`, the result of a `WM_POINTER*` message) is answered "not handled" when
   it arrives reentrantly, and its queued delivery is de-duplicated against the follow-up the OS
   sends for an unhandled event.
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
  `deadlines_fire_on_event_time_without_wall_clock_reads` in `flui-interaction`'s tests;
  `replay_determinism_holds_for_a_fixed_application_state` pins the scope: the same recording
  over a tree a non-input source changed between Down and Move is not claimed to replay.
- `input_sequence_ids_are_never_reused` and `an_exhausted_sequence_mint_refuses_permanently`,
  through a test-only mint started near the terminal value.
- `a_native_event_during_a_callback_is_queued_not_dropped` (headless); the Win32 and AppKit
  nested-pump rows and `a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once`.
- `latch_and_hover_are_keyed_by_surface_and_device`, `contactless_sequences_get_binding_minted_devices`
  and, in `flui-runtime`, `accessibility_input_reaches_the_semantics_owner`.
- Static assertions: `GestureBinding: !Send + !Sync`; `HostInput`, `InputSequenceId`,
  `InputSurface` and `ScrollStep` are `Send + 'static`. trybuild `gesture_binding_stays_on_its_thread`
  in `crates/flui-interaction/tests/compile_fail/`, and `compile_pass` rows accepting
  `Rc`-capturing recognizers and scroll connections.

Linux CI runs every row except the Win32 and AppKit nested-pump rows, which run on a native host
only. Per-thread Win32 runtimes and cross-thread capture are unverified until measured on Windows.

## Alternatives rejected

- **Splitting the platform thread from the owner thread.** Every synchronous answer the OS needs
  (`default_prevented`, TSF pulls, DirectManipulation `SetContact`, web `preventDefault`) would
  become late; Flutter merged these threads on every platform (threading research, §3).
- **Compositor-side scrolling now.** It requires declarative-only chains; Chromium, Firefox APZ and
  WinUI InteractionTracker either fall back to the main thread or drop scroll-linked callbacks.
- **Separate sequence ids per subsystem.** A fling handed between channel members, a haptic tied
  to the gesture and a recording row would then need translation tables.
