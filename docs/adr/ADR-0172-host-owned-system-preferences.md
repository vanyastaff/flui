# ADR-0172: Host-owned system preferences and ordered runtime delivery

- **Status:** Accepted architecture; implementation and consumer acceptance pending.
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

Gesture timing, mouse rectangles, touch slop, fling speed and wheel steps retain
their distinct meanings and units. A double-click rectangle's full width is not
the same measurement as displacement allowed on either side of an origin.
Native pixel measurements retain their coordinate context until projected for
the consuming presentation. A backend may query metrics for that presentation's
DPI without creating another native observer. Wheel disabled, line/character and
page values remain distinguishable; pixel-based input does not acquire a second
system multiplier.

`GestureArenaScope` distributes the interaction projection alongside its existing
arena identity. Its production consumers observe changes through lifecycle
dependencies. A settings update has an explicit active-sequence policy, pinned
through recognizer behavior: retain the admitted settings until terminal, or
replace/cancel through existing containment. Silently changing an active
sequence's thresholds is not an implementation choice. Authored overrides remain
authoritative. A separate gesture-settings source or parallel inherited scope
would recreate the authority this decision removes.

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

`MediaQuery` exposes widget-facing values with field-specific dependencies.
Its contrast projection uses normal contrast when the observation is unavailable;
the raw snapshot retains `None`. A failed native refresh preserves the prior
observation instead. Nested media providers may override that projection.
Cupertino resolves its authored palette variants using contrast independently
of explicit theme brightness. Material selects an optional contrast palette for
the effective light/dark family, then falls back to that family's ordinary
selection. These are palette-selection policies, not forced recoloring or a
claim to reproduce the operating system's custom high-contrast colors.

The runtime owns its root publication mechanism. Remove `AccessibilityFeatures`
and other superseded authorities when their consumers have migrated; no public
constant-backed facade or mount-only seed satisfies this decision.

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
