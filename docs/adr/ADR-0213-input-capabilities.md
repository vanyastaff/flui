# ADR-0213: Input availability comes from handles; `InputCapabilities` covers only what has no handle

- **Status:** Accepted (2026-10-13). Implemented: `InputCapabilities`, `Availability` and `PointerKindSet` in `flui-platform-api::input_capabilities`; the snapshot-then-changes delivery and its recovery rules in `flui-platform`'s `WindowCallbacks`; every backend's availability table in `flui-platform/src/shared/input_availability.rs`; `PlatformCapabilities` removed; the `unstable-input` re-exports in `flui-sdk` and the facade. Pending: the per-backend availability rows of `assert_backend_conforms` (ADR-0215), the recording round trip of `PlatformInput::Capabilities`, presence changes from the native Win32, AppKit, UIKit, Android and web backends (they report their initial table; the headless and winit backends report changes), and Win32 DirectManipulation, until which Windows reports precise scrolling and trackpad gestures `Unavailable`; the last two land with the native backend work.
- **Date:** 2026-10-13
- **Amends:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) §5: adopts the removal of
  `PlatformCapabilities` and `Platform::capabilities`, and names what replaces them.
- **Related:** [ADR-0210](ADR-0210-two-tier-haptics.md) (haptics availability through its
  handle), [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (IME through its handle),
  [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md),
  [ADR-0084](ADR-0084-open-capability-seam-and-plugins.md); the
  [interaction completeness spec](../plans/specs/flui-interaction/completeness/design.md) §6, §7.

## Context

`PlatformCapabilities` (then in `flui-platform`'s `traits/capabilities.rs`, now removed) was a per-platform
constant answering `supports_touch`, `supports_mouse`, `supports_stylus`,
`should_coalesce_pointer_moves`, `default_target_fps` and five more. It was wrong in both
directions: "Windows supports touch" says nothing about whether a touchscreen is attached, and
"Linux supports pen" is true on Wayland and false on X11 under the same backend. Its only users
are a `flui-platform` example and doc comments; ADR-0082 §5 already proposed deleting it.

Meanwhile a widget does need to know a few things. Should a drawing surface offer pen
pressure? Does the backend deliver phased precise scrolling, or only wheel detents? Will
trackpad pinch and rotate arrive as `PanZoom`? Can system-gesture exclusion regions be
honored? Capabilities that have a handle already answer through it: `PlatformWindow::haptics()`
returning `None` (and `HapticCapabilities`, ADR-0210), `text_input()` for IME.

## Decision

1. **Only handle-less capabilities.** `InputCapabilities` (`flui-platform-api`) describes pen
   input, precise (phased) scrolling, trackpad gestures (pinch and rotate as `PanZoom`), input
   regions and outgoing drags to the OS. Haptics and IME keep answering through their handles;
   nothing is reported twice. `outgoing_drag()` is the one authoritative answer for starting an
   OS drag session; [ADR-0211](ADR-0211-unified-drag-and-drop.md)'s `DragError::Unavailable` is
   the matching error.

   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   pub struct InputCapabilities { /* private */ }
   impl InputCapabilities {
       pub fn builder() -> InputCapabilitiesBuilder; // used by backends
       pub fn pen(&self) -> Availability;
       pub fn precise_scroll(&self) -> Availability;
       pub fn trackpad_gestures(&self) -> Availability;
       pub fn input_regions(&self) -> Availability;
       pub fn outgoing_drag(&self) -> Availability;
       pub fn present(&self) -> PointerKindSet; // devices attached now (item 2)
   }
   #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
   pub enum Availability { Native, Emulated, Unavailable }
   ```

   Backends build a value through `InputCapabilities::builder()`. The fields are private so that
   adding a capability is not a breaking change; they do not prove which backend produced a
   value. Getters carry no `get_`.
   `Availability` is a closed enum: `Native` is delivered by the OS path; `Emulated` is
   synthesized by the backend from other input (Web ctrl+wheel decoded as `PanZoom`) and may be
   ambiguous; `Unavailable` means this backend on this OS session has no path (pen, trackpad
   gestures and outgoing drags on X11).
2. **Implemented is not present.** `Availability` states what the backend implements. Whether a
   matching device is attached is a separate answer, `present()`, taken from the backend's
   `DeviceRegistry` (`flui-platform/src/shared/`), the same registry that assigns `DeviceId`s,
   so presence and the device ids in events cannot disagree. `Native` with no pen attached is a
   normal state.
3. **Snapshot, then changes, as events.** Every backend computes an initial
   `PlatformInput::Capabilities` for a surface when it is created, reflecting the devices
   already attached, retains it, coalesces capability changes into that pending snapshot only until the first ordinary input is queued; from then on capability transitions are queued in order with that input, so a pen that produced a Down and was then detached reports "present" before the Down and "absent" after it. Delivery is reentrancy-safe: installation commits the callback registration, then takes the pending snapshot and queued entries out of the backend's state one at a time, releasing every borrow and lock before each callback invocation; the not-yet-delivered tail stays in the backend's queue, ahead of input admitted during the callback, so a callback panic contained at the backend's callback boundary leaves the tail queued in its original order; an ordinary input entry that panicked is not re-delivered, but the capability snapshot is idempotent and stays at the head until a callback call for it returns normally, so no input is ever delivered before its baseline; the backend schedules an owner-thread wake that resumes the drain, so the tail does not wait for unrelated input. Each pass drains only the entries present when it started; input admitted reentrantly during a pass waits for the next pass, which the same wake schedules, so a callback that admits input on every call cannot monopolize the owner thread. Retries of the snapshot by one registration are bounded (three failed passes); after that the snapshot and the queued input stay held, with no further wakes, until the callback is re-registered or the surface closes (rows `a_panic_on_the_initial_snapshot_keeps_the_queued_tail_in_order` with an otherwise idle application, `the_snapshot_is_retried_before_any_queued_input`, `reentrantly_admitted_input_waits_for_the_next_pass`); a callback that re-registers, closes the window or releases its owner during that first delivery sees the committed state, and an outgoing callback is retired only after all guards are released, never restored over its replacement, and under containment custody: a panic while destroying its captures is contained separately, never replaces the first failure, and neither the committed replacement, the queued tail nor the scheduled recovery wake is lost to it (rows: replacement and owner release from the first delivery, `a_replaced_input_callback_whose_drop_panics_still_delivers_the_tail`). The snapshot is delivered as the first event when the surface's input
   callback is installed (`on_input` is registered after `open_window` returns), before
   draining any ordinary input; a snapshot is never emitted into an absent callback. After that, attaching or removing a device, or
   a session change that alters availability, is carried by the vocabulary variant `PlatformInput::Capabilities(
   InputCapabilities)` through the window's existing input path, so it is part of `HostInput`,
   ordered with the input around it and recorded. There is no polling API and no new callback
   registration, so no new `+ Send` registration is introduced.
4. **`PlatformCapabilities` is removed**, with `Platform::capabilities`,
   `OwnerPlatform::capabilities` and the per-backend structs. Its other questions belong
   elsewhere: lifecycle to `WindowExecutionState`, frame pacing to the presentation, move
   coalescing to the binding's coalescer, device kinds to presence (item 2).
5. **Gate.** `flui-platform-api` and `flui-platform` always build `InputCapabilities` and
   `Availability` for internal use, so Cargo's feature unification cannot change them. The
   user-facing gate is the additive `unstable-input` feature on the `flui-sdk` and facade
   re-exports, kept until every backend's rows in `assert_backend_conforms` pass.
6. **`on_input` loses `+ Send`** when owner-thread registration lands, as ADR-0082 §4 step 2
   requires (every backend storing callbacks owner-only and registration reachable only through
   owner-proof types); the capability event rides that path and needs no bound of its own.

## Consequences and verification

- Tests through public API: `input_capabilities_report_implemented_and_present_separately`
  (a backend implementing pen with no pen attached, then attached, then removed, each change
  delivered in order with surrounding input); `capability_change_is_ordered_with_input`;
  `devices_attached_before_the_window_opens_are_in_the_initial_snapshot`;
  availability rows per backend in `assert_backend_conforms` (pen, precise scroll, trackpad
  gestures, input regions, outgoing drag, each `Native`, `Emulated` or `Unavailable` as that backend claims); a
  `compile_fail` doctest that builds `InputCapabilities` with a struct literal outside
  `flui-platform-api` (struct literals only; the builder is public); a recording round trip of
  `PlatformInput::Capabilities`.
- The headless backend's rows, and every backend's availability table as pure functions, run in
  CI (Linux and wasm32 execute; Windows, macOS, Android and iOS are type-checked only). Device
  attach and removal on real hardware run on a native host through
  `cargo xtask device <os>-input`.
- Removing `PlatformCapabilities` touches one example and the `flui-platform` docs; nothing
  outside the crate calls it.

## Alternatives rejected

- **One capability struct for everything, haptics and IME included.** Two sources of truth: the
  handle can be `None` while the struct says supported.
- **Booleans.** They cannot say "emulated", which is what the Web pinch and X11 gaps need.
- **A capability query per platform constant** (today's trait). It describes the OS, not the
  session or the devices attached to it.
