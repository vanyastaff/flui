# flui-cupertino Architecture

The official Cupertino catalog builds on `flui-sdk`. Widget configuration
and callbacks are owner-local; the package does not own a reactive graph.

## Mapping decisions

### Event callbacks borrow the originating dispatch's write context

`CupertinoButton` press/long-press and `CupertinoTabBar` selection handlers
take `&mut EventCx` and return an `EventOutcome` (ADR-0086). Gesture wrappers
forward their context without opening a new writer. `CupertinoTabScaffold`
updates its controller before forwarding the same context and selected
index to the caller. Tab builders remain read-only queries.

The button's press animation still starts before its callback. The integration
test `tap_callback_writes_a_signal_and_rebuilds_its_reader` drives the mounted
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
`Semantics` wrapper publishes that same state along with the button role,
including for disabled buttons whose text label merges into the node. This
uses the existing `Semantics::enabled` contract also consumed by `RawButton`;
no callback conversion or separate accessibility state is introduced.

The `component_contracts` family checks the labelled tap button,
`long_press_only_button_announces_enabled`, and
`disabled_button_announces_disabled` through the emitted accessibility tree.
