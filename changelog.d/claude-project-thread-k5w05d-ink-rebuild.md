### Changed

- **An `InkWell` no longer rebuilds on hover, press or focus** (`flui-material`): while its ink
  is showing, a state change moves the state layer and the ripple at paint. The well rebuilds
  only to show its ink and to drop it once the ink is at rest. Each Material control built on a
  well mounts with fewer bytes; the counts are in `packages/flui-material/perf/baseline/`.
