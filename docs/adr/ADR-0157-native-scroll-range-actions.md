# ADR-0157: Native scroll adjustment shares viewport activity and physics

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** ADR-0155

## Decision

A Scrollable publishes an explicit ScrollView role, its live offset and bounds,
and a child position adjuster with a numeric range. AccessKit treats ScrollView
as read-only; the adjuster uses its supported Slider role and the label
`Scroll position`. Both semantics proxies listen to the shared ScrollPosition
and invalidate semantics, without rebuilding the viewport or its children.
At a boundary it withdraws only the directions that cannot move; a later offset
or dimensions notification republishes them.

The range retains its foundation `Axis` through render configuration, owned
semantics snapshots and exported node data. AccessKit writes horizontal ranges
to X and vertical ranges to Y; it publishes no orthogonal range. Existing authored
scalar metadata without an axis keeps its vertical fallback. Merging a child's
range inherits its axis with the range values instead of relabelling an existing
parent range. `scrollable_accessibility_ranges_follow_the_actual_axis` checks
both axes and subsequent offset publication through the mounted widget's native
accessibility payload.

Directional requests, native increment/decrement and native numeric SetValue
all enter the same owner-local movement path. Movement interrupts the old
trajectory, raises scroll activity and its direction, and applies the offset.
The configured physics then settle it, including PageView snapping and the
presentation's device-pixel tolerance. A move without a ballistic simulation
ends its activity after the consuming frame.

AccessKit 0.25.1 adapters do not produce directional scroll requests. The
supported native route is UIA RangeValue on Windows, numeric accessibility value
on macOS, and AT-SPI Value on Linux; increment/decrement are exposed where the
adapter supports them. ScrollView prevents the viewport being filtered as a
generic container. This does not claim native UIA ScrollPattern support.

## Validation

Mounted scroll semantics exercise the same shared position, updated action set,
numeric range and page settlement. The Windows accessibility probe drives the
concrete Scrollable through UI Automation and reads its offset back. Other
native adapters require their respective platform smoke runs.
