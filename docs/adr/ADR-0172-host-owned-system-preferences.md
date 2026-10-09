# ADR-0172: Host-owned system preferences and ordered runtime delivery

- **Status:** Accepted architecture; gesture/wheel consumers implemented with local acceptance; broader implementation and acceptance pending.
- **Date:** 2026-10-08
- **Supersedes:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §4 only.
- **Related:** [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0171](ADR-0171-ui-runtime-and-host-vocabulary.md).

System preferences outlive individual windows and affect independent UI runtimes.
Reading them through a representative window couples unrelated lifetimes and can
apply one monitor's geometry to another. Separate gesture, text and motion
sources would duplicate native observers and give consumers inconsistent updates.
One host therefore owns the native observation lifetime and accepted preferences;
each consumer derives its representation from the same accepted state.

## Scope and ownership

The contract remains in `flui-platform-api`, native producers in `flui-platform`,
and host composition in `flui-app`. This decision authorizes system-preference
production, delivery and consumer integration with those existing crate names.
The other decisions proposed in ADR-0151 through ADR-0154 retain their own status.

`SystemPreferences` is an immutable, validated value. It describes text sizing,
contrast, bold text, ordered preferred languages, motion, gesture timing/geometry
and wheel preferences. Fields whose values are unavailable retain that fact;
framework fallback values belong to consumer policy. Failed refresh and an
unsupported observation are distinct outcomes. A failed refresh preserves the
last accepted value and its pending delivery.

Text-scale admission supports `1/64..=64`, inclusive, as a deliberately broad
accessibility range rather than arbitrary geometric zoom. Merely requiring a
positive finite `f64` permits observations that overflow or underflow downstream
`f32` shaping. Out-of-range observations fail validation, preserving the normal
source failure policy; they are not silently clamped to a different preference.
This bounds the system multiplier, not every possible authored text metric.

The host owns one observation set, possibly using several native notification
mechanisms. It lives before the first user window and ends with that host
incarnation. Brightness, geometry and safe-area remain presentation-specific.
Native observer handles and callback stacks never enter runtime values.

## Admission, delivery and retirement

1. Register native observers before sampling initial state. Sampling does not
   imply an atomic OS transaction across independent getters. An invalidation
   arriving during sampling retains another refresh obligation; repeated changes
   must not force an unbounded synchronous sampling loop.
2. Validate and commit an owned snapshot before notifying or waking its consumer.
   Accepted state and outstanding delivery are separate. Absent, replaced or
   panicking hooks cannot erase accepted state; an older successful notification
   cannot acknowledge a newer obligation. Callbacks and outgoing captures retire
   outside locks and owner-local borrows, under the existing first-failure policy.
3. Foreign-thread notifications admit data or invalidation and wake the owner.
   Runtime delivery executes on the owner through the typed host FIFO. Adjacent
   compatible preference updates may coalesce; input, frames, lifecycle and other
   observable work preserve their admitted order. Native invalidation itself is
   not a snapshot and does not promise the OS state at an earlier input timestamp.
4. Seed each runtime before mounting its first root. A later runtime receives the
   host's latest accepted revision; later presentations receive their runtime's
   latest revision. Queued older work cannot overwrite a newer seed. The host
   retains delivery to each admitted live recipient independently, so a closed or
   failing sibling cannot discard the remaining recipients.
5. Shutdown closes admission before native observer removal. Late callbacks are
   inert for that incarnation and cannot reach a replacement host. Subscription
   revocation failures retain enough ownership to prevent a native use-after-free;
   they do not authorize calling UI code after shutdown. Revisions and identities
   never wrap into a previously valid identity.

## Projection and consumer policy

Motion consumer integration remains pending: Windows observes the setting and
the host distributes it, but the runtime does not yet apply it to animations.
The owner retains this observation API for the upcoming animation-policy work;
that follow-up must wire the policy and demonstrate active-animation updates,
authored overrides and unknown-value fallback through production consumers.
Snapshot delivery alone is not reduced-motion support.

Gesture timing, mouse rectangles, touch slop, fling speed and wheel steps retain
their distinct meanings and units. A double-click rectangle's full width is not
the same measurement as displacement allowed on either side of an origin.
Native pixel measurements retain their coordinate context until projected for
the consuming presentation. A backend may query metrics for that presentation's
DPI without creating another native observer. Wheel disabled, line/character and
page values remain distinguishable; pixel-based input does not acquire a second
system multiplier.

`GestureArenaScope` distributes a read-only, owner-local interaction projection
alongside its existing arena identity. Host publication commits this projection
before the next admitted input; a later inherited rebuild is not its delivery
barrier. Each new contact or gesture session captures an immutable settings
snapshot. Active contacts, multi-contact handoffs and consecutive-tap candidates
retain their admitted settings until terminal. Changing the host observation
therefore affects new sequences without silently changing an active sequence's
thresholds. Terminal fling policy also comes from that admitted snapshot rather
than a later settings read or a framework default.

Drag release and scale focal release retain their raw measured velocity for
callbacks and expose a separate admitted velocity for inertial consumers. Focal
velocity is measured in logical pixels per second and uses the admitted fling
range. Scale-change velocity is measured in scale units per second and does not
acquire a pixel-speed limit.

A native Begin captures the profile when its session is admitted, independently
of recognizing the first movement. Refused admission, an admitted dormant
session and recognized delivery are distinct outcomes. A Begin refused while
the actor owns touch contacts cannot later become a new session from its tail;
its terminal event clears that refused stream. An Update received without any
Begin retains the independent relative-step contract.

Velocity estimation is consumer policy, not an observed OS preference. The
framework uses least squares for absolute pointer-position gestures on every
platform; authored scopes may select another estimator, and each admitted
contact or native session retains that selection. This keeps one tested baseline
without asserting native fling parity from a platform name. Android's
[axis-specific default strategy](https://android.googlesource.com/platform/frameworks/native.git/+/refs/heads/main/libs/input/VelocityTracker.cpp)
uses least squares for X/Y and impulse for differential scroll input; that
differential policy does not apply to absolute gesture-position samples. The
other estimators are configurable strategies, not native system observations.

Authored scope settings remain authoritative. An actual authored provider
replacement commits new recognizer ownership before cancelling outgoing owners
through existing containment; an equal fixed profile or an unchanged live
provider identity does not cancel a gesture. Lifecycle dependencies handle these
provider replacements, while ordinary host updates use the existing live
projection. A separate native settings source or parallel inherited scope would
recreate the authority this decision removes.

An unconfigured nested gesture scope inherits both projections. Explicit
authored settings override gesture policy without hiding the host's wheel
policy. The composition wrapper resolves these values into one inherited
provider; it does not install a second settings authority.

Presentation geometry queries distinguish accepted absence from failure.
Accepted absence restores the consumer baseline. A failed query retains the
last observation and a bounded retry obligation. Its geometry is usable only
in the coordinate context it was accepted for: after a DPI change, new
admissions use the baseline until a query for that presentation succeeds.
Timing observations remain independently applicable. Retrying services the
owner's existing wake path without beginning a synthetic frame.

A successful exact-context query can still yield geometry whose projection
against the authored baseline is not representable. This acknowledges the query
obligation, but does not replace the last successfully projected geometry in that
same context; without one, the consumer uses its baseline. Latest timings and
wheel observations remain independently deliverable. Repeating a successful
native read cannot repair a deterministic projection failure, so only a later
host or DPI barrier initiates another query. The raw host snapshot retains the
observation; the fallback is consumer policy.

Native touch slop supplies the touch displacement measurement. The consumer
retains its deliberate pan-to-hit and per-axis policy ratios rather than
collapsing distinct gesture thresholds into one value. Projection validates
intermediate arithmetic, handles a zero baseline explicitly and leaves
dimensionless scale tolerance independent of pixel distances.

An observed mouse double-click interval measures first press to second press;
it does not extend by the duration of the first held press. This follows the
[Windows double-click message sequence](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-mouse-input#double-click-messages)
and [AppKit mouse-down click counting](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingMouseEvents/HandlingMouseEvents.html).
Touch double-tap and fixed authored timing keep their existing release-to-press
policy. A retained consecutive-tap candidate includes its timing origin.

Motion observations distinguish no preference, reduced motion and a finite,
strictly positive duration scale. An OS scale of zero maps to reduced motion;
one maps to no preference; negative and non-finite values are refused. Duration
scale and playback rate are different quantities. Application motion policy and
the behavior of active animations belong to the runtime/animation consumer.

Text observations and authored font sizes remain distinct from resolved layout
sizes. `MediaQuery` and actual text/editing consumers must update together without
scaling authored styles twice. The font-sizing policy must document its behavior
for different authored sizes and its native mapping. This decision does not adopt
another framework's scaler interface, select an unverified interpolation curve,
or assert native typography parity from a scalar or sparse samples. Resolving
that policy and exercising native producers remain part of implementation
acceptance; repairing existing linear consumers does not complete it.

Editable caret defaults follow the shaped single-line height, shared by paint
and collapsed-range/IME geometry. An explicit logical caret height stays fixed;
removing the override restores the default. Widget composition retains that
distinction instead of converting the default to an explicit fixed length.

Presentation DPR is accepted before publication: direct updates reject invalid
ratios without changing render or inherited state; native metrics discard an
invalid ratio before coalescing or resizing the surface. Initial inherited data
uses the ratio accepted by the pipeline, including its fallback on rejection.

`MediaQuery` exposes widget-facing values with field-specific dependencies.
Its contrast projection uses normal contrast when the observation is unavailable;
the raw snapshot retains `None`. A failed native refresh preserves the prior
observation instead. Nested media providers may override that projection.
Cupertino resolves its authored palette variants using contrast independently
of explicit theme brightness. Material selects an optional contrast palette for
the effective light/dark family, then falls back to that family's ordinary
selection. These are palette-selection policies, not forced recoloring or a
claim to reproduce the operating system's custom high-contrast colors.

The runtime owns its root publication mechanism. The constant-backed
`AccessibilityFeatures` type and its unused app storage are removed; they had
no consumers to migrate. Other superseded authorities are removed with their
consumer migrations. No public constant-backed facade or mount-only seed
satisfies this decision.

## Gesture and wheel consumer acceptance

The implemented projection reaches real recognizers and wheel/inertia consumers.
[`admitted_gesture_settings_contract`](../../crates/flui-interaction/tests/recognizer_api/settings_admission.rs)
and [`gesture_lifecycle_matrix`](../../crates/flui-interaction/tests/gesture_lifecycle.rs)
pin immutable admission policy and selected estimators.
The widget tables `pointer_and_gesture_recognition`, `scroll_physics_and_activity`
and `navigator_and_overlay` in [consumer contracts](../../crates/flui-widgets/tests/contracts.rs)
exercise provider replacement, native Begin admission and admitted fling policy.
[`owner_metrics_contract`](../../crates/flui-runtime/tests/contracts/owner_metrics.rs)
checks ordered publication, mounted wheel/inertia delivery and independent
presentation geometry recovery.
[`frame_pacing_and_pump_matrix`](../../crates/flui-runtime/src/ui_runtime/tests/mod.rs)
additionally pins
successful-query acknowledgement when projection is unrepresentable, preserving
same-context geometry or a safe baseline. Independent inverses of the geometry
repairs failed these affected contracts; restored implementations passed.

Windows [`preferences_contract`](../../crates/flui-platform/tests/preferences.rs)
executes native queries and cold-cache recovery through
`windows_reads_preferences_before_a_user_window_exists`. The direct child rows
`native_mouse_wheels_keep_hover_identity_and_signed_units` and
`fractional_native_wheel_packets_preserve_observed_precision_and_source` in
[`test_window_lifecycle_contract`](../../crates/flui-platform/tests/contract.rs)
also executed successfully, without `CANNOT_VERIFY`: actual injected and queued
native packets preserve source, precision, signed Detents and DPI conversion.
Interaction all-target/all-feature clippy and all 12 compiler fixtures passed.
Android and AppKit evidence is Rust-only library compilation, not native
execution. The scoped `cargo xtask check-changed` gate passed at `38f9d5238`
against `b357bc903`: 522 tests passed, 20 skipped, and platform compiler guards
passed. Strict clippy, private-items rustdoc, doctests, native Windows and wasm
checks passed, together with both 65-case per-feature passes. Complete Apple and
Android cross-typechecks need unavailable SDKs/toolchains on this host; the
Linux native suite needs xvfb-run. Those paths and CI acceptance remain pending.
This local gesture/wheel acceptance does not complete text, motion or
the broader host-authority acceptance in the platform-layer specification.

## Windows transport constraint

Windows uses an owned hidden top-level receiver for broadcast setting changes.
It stays outside user-window membership, frame production and exit policy.
Existing message-only control HWNDs retain their purpose: Windows documents that
[message-only windows do not receive broadcasts](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#message-only-windows).
The source samples the needed values after
[WM_SETTINGCHANGE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-settingchange).
WinRT observers use a scoped STA entry compatible with the platform's COM STA;
every successful [RoInitialize](https://learn.microsoft.com/en-us/windows/win32/api/roapi/nf-roapi-roinitialize)
is balanced while preserving the platform's outer COM lifetime.

Native callback execution, headless protocol behavior, cross-platform compilation
and actual OS notification delivery are separate evidence. A probe that registers
and removes a token does not establish change delivery. Implementation acceptance
is tracked in the [platform-layer spec](../plans/specs/platform-layer/tasks.md).

The Windows winit transport uses this same owner-local source. It publishes a
sampled preference observation before input or wake delivery, preserves deferred
sampling for its deadline retry and does not dirty user windows merely to retry
preferences. Shutdown closes the owner signal and releases the source before
retiring user-window callbacks. Raw wheel input remains signed Detents; consumers
resolve the current system count once.

The native Windows winit tests
`windows_winit_wheels_preserve_raw_units_and_observe_system_policy` and
`windows_winit_cold_preferences_recover_on_an_idle_owner_turn` executed the actual
wheel packet pipeline, cold deferred bootstrap followed by idle-owner recovery,
WM_SETTINGCHANGE refresh failure followed by idle-owner recovery, retry without
an extra redraw and refusal after shutdown. These witnesses do not change system
policy. The retry witness measures delivered native paint callbacks on an
offscreen, unactivated user window; preference-only recovery leaves that count
unchanged. They establish this Windows transport's observed wheel and preference
delivery, not physical pen or touch input, other operating-system execution or
broader platform-layer acceptance.
