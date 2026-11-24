### Fixed

- `InspectorCounters` counts rebuilds caused by `RebuildReason::SignalChange` (`flui-devtools`): its per-cause table had room for ten causes of the eleven, so `InspectorSnapshot::rebuilds_for(RebuildReason::SignalChange)` stayed at zero.
