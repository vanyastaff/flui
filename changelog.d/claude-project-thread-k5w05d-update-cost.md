### Fixed

- **`flui-engine`**: the recording cache no longer collects and sorts every kept recording
  in each frame that leaves it over its 64 MiB budget: it evicts the least recently rastered
  recordings in time proportional to what it evicts, so a frame that records one chunk beside a
  full cache costs what it did before the cache. A capture allocates each recording's arenas to
  their length, and the recordings of a scene owner that no window or offscreen slot draws any
  more (its slot detached or draws another owner's version) are dropped in that frame instead of
  waiting for the budget.
