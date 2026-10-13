# ADR-0213: Input availability comes from handles; `InputCapabilities` covers only what has no handle

- **Status:** Accepted (2026-10-13); implemented by the interaction completeness spec
- **Date:** 2026-10-13
- **Amends:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) §5: adopts the removal of
  `PlatformCapabilities` and `Platform::capabilities`, and names what replaces them.
- **Related:** [ADR-0210](ADR-0210-two-tier-haptics.md) (haptics availability through its
  handle), [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (IME through its handle),
  [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md),
  [ADR-0084](ADR-0084-open-capability-seam-and-plugins.md); the
  [interaction completeness spec](../plans/specs/flui-interaction/completeness/design.md) §6, §7.

## Context

`PlatformCapabilities` (`crates/flui-platform/src/traits/capabilities.rs`) is a per-platform
constant answering `supports_touch`, `supports_mouse`, `supports_stylus`,
`should_coalesce_pointer_moves`, `default_target_fps` and five more. It is wrong in both
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
   input, precise (phased) scrolling, trackpad gestures (pinch and rotate as `PanZoom`) and input
   regions. Haptics and IME keep answering through their handles; nothing is reported twice.

   ```rust
   #[non_exhaustive]
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   pub struct InputCapabilities { /* private */ }
   impl InputCapabilities {
       pub fn pen(&self) -> Availability;
       pub fn precise_scroll(&self) -> Availability;
       pub fn trackpad_gestures(&self) -> Availability;
       pub fn input_regions(&self) -> Availability;
       pub fn present(&self) -> PointerKindSet; // devices attached now (item 2)
   }
   pub enum Availability { Native, Emulated, Unavailable }
   ```

   The fields are private, so a value is only produced by a backend; getters carry no `get_`.
   `Availability` is a closed enum: `Native` is delivered by the OS path; `Emulated` is
   synthesized by the backend from other input (Web ctrl+wheel decoded as `PanZoom`) and may be
   ambiguous; `Unavailable` means this backend on this OS session has no path (pen and trackpad
   gestures on X11).
2. **Implemented is not present.** `Availability` states what the backend implements. Whether a
   matching device is attached is a separate answer, `present()`, taken from the backend's
   `DeviceRegistry` (`flui-platform/src/shared/`), the same registry that assigns `DeviceId`s, so presence and the
   device ids in events cannot disagree. `Native` with no pen attached is a normal state.
3. **Change is an event.** Attaching or removing a device, or a session change that alters
   availability, is delivered through the window's existing input path as a new
   `InputCapabilities` value, ordered with the input around it. There is no polling API and no
   new callback registration, so no new `+ Send` registration is introduced.
4. **`PlatformCapabilities` is removed**, with `Platform::capabilities`,
   `OwnerPlatform::capabilities` and the per-backend structs. Its other questions belong
   elsewhere: lifecycle to `WindowExecutionState`, frame pacing to the presentation, move
   coalescing to the binding's coalescer, device kinds to presence (item 2).
5. **Gate.** `InputCapabilities` and `Availability` sit behind the additive `unstable-input`
   feature of `flui-platform-api`, re-exported by `flui-sdk` and the facade under the same
   feature, until every backend's rows in `assert_backend_conforms` pass.
6. **`on_input` loses `+ Send`** when owner-thread registration lands, as ADR-0082 §4 step 2
   requires (every backend storing callbacks owner-only and registration reachable only through
   owner-proof types); the capability event rides that path and needs no bound of its own.

## Consequences and verification

- Tests through public API: `input_capabilities_report_implemented_and_present_separately`
  (a backend implementing pen with no pen attached, then attached, then removed, each change
  delivered in order with surrounding input); `capability_change_is_ordered_with_input`;
  availability rows per backend in `assert_backend_conforms` (pen, precise scroll, trackpad
  gestures, input regions, each `Native`, `Emulated` or `Unavailable` as that backend claims); a
  `compile_fail` doctest that constructs `InputCapabilities` outside `flui-platform-api`.
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
