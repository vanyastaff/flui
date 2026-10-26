### Fixed

- **`flui-native`**: Windows pen and touch samples coalesced from one `WM_POINTER*` message
  now carry distinct, increasing times taken from the reading's performance counter, instead of
  the ~15.6 ms system tick they shared, so fling velocity from high-rate digitizers is no longer
  computed from zero or jumping intervals.
