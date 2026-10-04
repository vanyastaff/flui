### Removed

- Removed Android's unused page-aligned allocation helpers and `PageAlignedVec`, whose panic during element retirement could cause repeated destruction. Framework buffers use standard owning containers and wgpu manages GPU allocations.
