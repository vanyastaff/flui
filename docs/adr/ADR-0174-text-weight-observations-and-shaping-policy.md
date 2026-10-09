# ADR-0174: Text-weight observations and shaping policy

- **Status:** Consumer integration implemented; native UIKit execution pending.
- **Date:** 2026-10-08
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md),
  [ADR-0092](ADR-0092-per-realm-text-over-parley.md).

## Decision

Preserve the meaning of an observed weight preference before applying framework
policy. `SystemPreferences::text_weight()` is an optional `TextWeightPreference`:
`NoPreference`, categorical `Bold`, or a nonzero signed numeric `Adjustment`.
Unknown remains absent; zero normalizes to `NoPreference`. A native producer
handles its unavailable-value sentinel before constructing a numeric adjustment.
This replaces the unused Boolean `bold_text` observation rather than introducing
two independently applicable observations.

The runtime projects the accepted snapshot to one inherited
`MediaQueryData::font_weight_adjustment`. Unknown and `NoPreference` project to
zero, a numeric request retains its signed value, and categorical `Bold` projects
to +300. The additive categorical mapping is FLUI policy, not a claim about
Apple's custom-font rendering. It preserves authored ordering until saturation
and does not lower an authored W900 to W700. An explicitly nested `MediaQuery`
remains a whole replacement and can author zero to preserve its subtree's weights.

Text, RichText and EditableText pass this adjustment separately from their
authored styles. The common Parley adapter applies it once after style inheritance
and before font matching and shaping. It resolves the last valid explicit `wght`
coordinate, otherwise the inherited authored weight, otherwise 400; adds in `f64`;
and bounds the requested result to 1..1000 before narrowing. Matching and explicit
weight-axis settings receive the same resolved value. Other valid axes retain
their authored values. Span diagnostics retain the resolved floating-point
weight without rounding it to the authored convenience enum. Invalid axes follow
the existing filtering policy. Zero
preserves the authored matching and axis path, including coordinates outside the
nominal CSS range. No authored span tree is rewritten.

`TextPainter` invalidates its layout cache when the adjustment changes. Its
`ParagraphSpec` carries the policy into committed layout, intrinsic measurement
and ellipsis reshaping. Paint, caret, selection and hit queries continue to use
the layout that produced the glyphs. The authored W100..W900 `FontWeight`
convenience enum stays unchanged; resolved weights remain continuous.

## Native sources

UIKit reads `UIAccessibilityIsBoldTextEnabled` on the main thread. The platform
registers `UIAccessibilityBoldTextStatusDidChangeNotification` before sampling,
independently of scene or user-window membership. Its notification block captures
only an admission flag and a weak existing owner signal. It performs no UIKit
read or UI callback; the owner samples current state on its next turn. The
temporary signal lease retires inside the existing owner panic boundary. Shutdown
closes admission before removing the retained observer token, with no `RefCell`
borrow across removal. A late callback cannot access a retired source or another
platform incarnation. Failed reads do not publish an off observation.

Other backends currently leave text weight unavailable. Android integration must
extend the host's existing Activity-context preference aggregate, using API 31's
`Configuration.fontWeightAdjustment` and treating
`FONT_WEIGHT_ADJUSTMENT_UNDEFINED` as unknown. It must retain numeric observations
and existing refresh/retry ownership. It is not implemented by this decision's
consumer integration. Windows non-client role fonts are not interpreted as a
global accessibility-weight preference.

## Evidence and limits

The mounted `bold_text_changes_the_painted_glyphs` row checks the actual registered
variable font instance in submitted frames for all three text widgets, numeric
bounds, on/off/unknown restoration, nested overrides, and the first frame of late
runtimes and presentations. Disabling only presentation-install seeding makes
the new presentation's distinguishable submitted text use the default instance;
restoring that seed passes. This does not establish simultaneous presentation
submission. The shaping row
`text_weight_adjustment_shapes_once_and_restores_authored_weights` checks mixed
span inheritance, explicit and duplicate weight axes, fractional and extreme
numeric adjustments, restoration, intrinsic widths and caret endpoints. Reverting
the shaping adjustment makes the mounted font-instance regression fail. The
same shaping row rasterizes vendored Roboto-Regular through Swash: adjusted ink
coverage grows and disabling the adjustment restores exact authored glyph masks.
Disabling the production adjustment makes that raster-coverage assertion fail.
The generated variable fixture pins font-instance coordinates; its empty
outlines do not establish a visible variable-font stroke change.

Local iOS checking and clippy establish type compatibility only. UIKit-generated
notifications, setting changes and observer retirement have not been executed on
an iOS device or simulator. Native Android observations remain pending. No
reduced-motion behavior is implied; Motion integration remains owner-deferred.

## API migration

This pre-1.0 change intentionally removes `bold_text()` and `with_bold_text(bool)`;
use `text_weight()` and `with_text_weight(TextWeightPreference)` instead. Public
`MediaQueryData` and `ParagraphSpec` literals gain `font_weight_adjustment`:
use zero to retain authored weight behavior. Repository literal migration is
audited with `rg -n 'ParagraphSpec|MediaQueryData|bold_text|with_bold_text' --glob '*.rs'`
over the workspace, including examples and tooling. A default-feature SemVer comparison with an explicit patch
release type reports these intended breaks: removed inherent methods in
flui-platform-api and added constructible fields in flui-painting and flui-widgets.
This is an intentional pre-1.0 API migration, not a compatible patch. Optional
feature and platform verification remain required before publication.

## References

- [UIKit Bold Text observation](https://developer.apple.com/documentation/uikit/uiaccessibility/isboldtextenabled)
  and [change notification](https://developer.apple.com/documentation/uikit/uiaccessibility/boldtextstatusdidchangenotification).
- [Android Configuration.fontWeightAdjustment](https://developer.android.com/reference/android/content/res/Configuration#fontWeightAdjustment).
- [Compose numeric interceptor, pinned AndroidX source](https://github.com/androidx/androidx/blob/cd5e293897b9b35e6d5984183806e0f10ac5c7b9/compose/ui/ui-text/src/androidMain/kotlin/androidx/compose/ui/text/font/AndroidFontResolveInterceptor.android.kt).

These sources distinguish categorical and numeric observations. They do not
establish a universal Apple numeric adjustment or replace FLUI's shaping contract.
