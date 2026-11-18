### Fixed

- **`flui-engine`**: the recording cache no longer collects and sorts every kept recording
  in each frame that leaves it over its 64 MiB budget: it evicts the least recently rastered
  recordings in time proportional to what it evicts. On lavapipe at 60 × 60 boxes the raster
  CPU time of a one-box update falls from 6.7 ms to 1.2 ms (1.1 ms before the cache), a first
  frame from 53 ms to 23 ms and a full update with no repaint boundary from 68 ms to 29 ms
  (13 ms each before the cache, since a capture still copies each recording it keeps). A capture
  allocates each recording's arenas to their length; the recordings of a scene owner that no
  window, offscreen slot or `Renderer` draws any more are dropped in that frame instead of
  waiting for the budget; and a cache emptied by a memory-pressure reclaim lets its room go.
