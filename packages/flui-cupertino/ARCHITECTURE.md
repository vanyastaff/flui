# flui-cupertino Architecture

The official Cupertino catalog builds on `flui-sdk`. Widget configuration
and callbacks are owner-local; the package does not own a reactive graph.

## Mapping decisions

### Button fade ownership follows its presentation clock

The button owns one controller, made at the first press that fades, across
both legs of its press fade. The fade follows the press: the tap's down
holds the button and its up or cancel releases it. A leg that is running is
never cut short; the leg that completes starts the next only when the held
side changed meanwhile, and a leg that ends cancelled starts nothing, so a
press never starts a stale release. A key activation or an assistive click
has no contact: it runs the handler and leaves the opacity as it is. A
scope replacement transfers the owner without restarting its displayed
sample; disposal withdraws the active owner, whose running leg then ends
cancelled.

`an_active_button_press_fade_migrates_and_retires` and
`an_active_button_release_fade_migrates_and_retires` exercise pointer taps,
mid-leg registry replacement, continuous compositing alpha, progress from the
new clock, resting frame demand and unmount during another active fade.
`the_press_fade_follows_the_held_contact_and_a_cancelled_press_restores_it`
holds, releases, moves out and cancels a press, and
`a_contactless_activation_runs_the_handler_without_a_fade` clicks it through the
accessibility tree.

### Typography defaults select numeric growth independently of authored fonts

Body and action styles use Body growth; small action uses Callout, tab labels
Caption2, navigation title Headline, large navigation title LargeTitle, and
picker/date-time picker Title2 (ADR-0183). Navigation action delegates to the
action style. These profiles resolve numeric size only; authored family,
weight, spacing and height remain intact.

The public `mounted_text_roles_paint_distinct_numeric_sizing_answers` row resolves
styles through a mounted CupertinoTheme and checks actual painted glyph sizes
and restored layout through MediaQuery publication. It does not establish
UIKit font identity, optical sizing or native execution.

### Palette resolution shares brightness precedence and selective dependencies

`CupertinoColors` is UIKit's palette as Flutter carries it, every value
Flutter's. Standalone dynamic colors, theme resolution and text roles pick a
variant through one private color resolver, from the effective brightness, the
interface level and the contrast preference. A theme's explicit brightness wins
even before that theme is installed as an ancestor; the level follows the
nearest `CupertinoUserInterfaceLevel`, base without one; contrast follows the
nearest `MediaQuery`. Static colors remain authored values. A standalone
dynamic color subscribes only to the inputs it varies along, so unrelated
window size or level changes do not rerun its resolution; a theme reads
brightness, level, contrast and the system accent once for all its colors.
Nothing in this package installs an elevated level: the dialogs and sheets
that do in Flutter are not in this catalog, and an application installs one
over what it presents above its main content.

`retained_colors_follow_contrast_and_explicit_brightness` and
`authored_and_nested_colors_ignore_outer_contrast` pin retained-tree behavior.
`explicit_app_brightness_applies_to_live_contrast_colors` checks the palette and
text roles an app's descendants read during live contrast changes.
`palette_constants_resolve_to_flutters_values` holds a sample of the palette to
Flutter's values, `an_elevated_level_picks_the_elevated_variants` the level's
selection, its live change and its selective dependency, and
`a_theme_below_an_elevated_level_resolves_apart` the theme resolution keyed by
the level.

### A theme is resolved once per set of inputs and read by field

`CupertinoThemeData` is what an author writes, each `None` the iOS default;
`ResolvedCupertinoTheme` is what widgets read, its colors concrete and its nine
text roles built. They are two types because a concrete color is a different
type from a dynamic one: a resolved theme cannot be resolved again or compared
with its source by mistake.

`CupertinoTheme` keeps the resolutions its readers asked for in a cache its
view value owns, so a clone of the view shares it. The key is every input
resolution reads: the effective brightness, the interface level, the contrast
preference and the system accent when it applies. Resolution under a key reads
nothing else from the context, so the key is complete by construction. The
cache holds eight entries, the brightness, level and contrast combinations,
and evicts the oldest. Readers under the same inputs share one `Rc`;
`CupertinoTheme::of` without an ancestor theme resolves the default afresh.
The default roles share one process-wide copy of their family names, which is
immutable data.

`CupertinoApp` publishes the authored data, not a resolution of it, so a nested
`MediaQuery` that changes brightness or contrast changes the theme below it.

`readers_under_one_context_share_one_resolution`,
`a_nested_media_query_resolves_the_app_theme_below_it` and
`every_text_role_set_on_the_theme_reaches_a_reader` pin the sharing, the nested
resolution and the nine settable roles.

### Event callbacks borrow the originating dispatch's write context

`CupertinoButton` press/long-press and `CupertinoTabBar` selection handlers
take `&mut EventContext` and return an `EventOutcome` (ADR-0086). Gesture wrappers
forward their context without opening a new writer. `CupertinoTabScaffold`
updates its controller before forwarding the same context and selected
index to the caller. Tab builders remain read-only queries.

The button's press animation starts before its callback, and the callback
runs with no borrow of the button's state held. The integration test
`tap_callback_writes_a_signal_and_rebuilds_its_reader` drives the mounted
button through pointer dispatch and checks the signal and subsequent rebuild.

### Tab selection uses the current bar's bounds

The controller stores selection without owning a tab count. A scaffold may
replace its bar while retaining that controller, so build validates its index
against the current items in every profile. An invalid selection recovers
through the build error boundary; it cannot silently hide every tab in release.
The standalone bar's selection setter rejects an out-of-range index immediately.

An immutable count on the controller would add a second count to synchronize
when the bar changes. A bounded index tied to one bar would likewise need
revalidation on replacement. The existing dynamic configuration contract keeps
that check at consumption instead of adding an unused public index type.

`navigation_contracts` includes
`out_of_range_controller_selection_reports_error_and_recovers` (invalid live
selection, subsequent valid selection and pointer delivery) and
`standalone_bar_rejects_an_out_of_range_selection`.

### Button availability reaches accessibility

A button is enabled when either its tap or long-press callback exists. Its
behaviours and its semantics are one interactive node (ADR-0322 §1), as
`RawButton`'s are: the node's description is the button role, its switch is
that enabled state, including for disabled buttons whose text label merges
into the node, and its press group offers the tap as the click and the long
press as the context menu. No separate accessibility state is introduced.
The same node hosts the button's focus node, draws the inherited focus ring
and binds `ActivateIntent` and `ButtonActivateIntent` to the press, as
`InkWell` does; the switch keeps a disabled button from taking focus and
leaves its bindings declared and switched off.

The `component_contracts` family checks the labelled tap button,
`long_press_only_button_announces_enabled`, and
`disabled_button_announces_disabled` through the emitted accessibility tree,
and `the_activation_key_presses_a_focused_button_and_a_disabled_one_takes_no_focus`
through a `Tester`'s Tab and Enter.

### A button's fill keeps its resolved alpha

A plain or filled button paints its colour as the ambient brightness and
contrast resolve it, alpha included; only a tinted button replaces the alpha,
with its fixed light or dark opacity. Flutter multiplies the fill by the
unresolved colour's opacity, the light variant's, which paints a dark-mode
`SEPARATOR` fill at alpha 73 instead of 153.
`a_plain_button_paints_its_resolved_colour_with_its_own_alpha` checks both
brightnesses.

### The app plays the shared parallax slide for its routes

`CupertinoApp` installs a `PageTransitions` whose page builder is
`flui-widgets`' `ParallaxPageTransition` (ADR-0221), so a page a `Router`
places, given no transitions of its own, slides in as a `CupertinoPage` does.
The slide is not this package's copy: Material's Apple-platform default plays
the same type, and official packages depend only on `flui-sdk`, so the one
shared implementation lives a layer down (ADR-0221 D4). Its leading-edge
shadow is a `DecoratedBoxTransition` on the route's own progress, painted only
while the page moves.

The builder carries `CupertinoPageRoute`'s chrome with it: a page that adopts
it pops on a drag from its leading edge, and the app sets its barrier to
Flutter's page-transition dim (`0x18000000`), which fades in under the page as
it enters and out as it leaves. Material's Apple-platform default keeps the
swipe and has no dim, as a `MaterialPageRoute` has none.

`cupertino_app_slides_a_page_in_from_off_the_right_edge_over_500ms`,
`cupertino_route_casts_its_edge_shadow_only_while_moving`,
`cupertino_route_does_not_slide_under_reduced_motion`,
`cupertino_app_page_pops_on_an_edge_swipe` and
`cupertino_app_dims_the_page_below_while_the_next_slides_over_it` pin the
slide and its chrome through `CupertinoApp::router`.

### The page control is the shared indicator in the iOS look

`CupertinoPageControl` holds a `flui-widgets` `PageIndicator` and sets only its style: 7 pt dots
9 pt apart, up to nine at once, the current page white and the others white at 30 %. Paging,
the window, the announced value, right-to-left order and reduced motion are the indicator's, so
the two cannot drift apart. `the_control_draws_ios_dots_and_turns_its_pager` pins the look and
the paging through this package's public entry point.
