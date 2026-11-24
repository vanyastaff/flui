### Changed

- A keyed list's reconcile allocates for the list, not for each child: the index of old children by
  key hash makes two allocations however many keys it holds, where it made one per key, and a
  removed subtree is walked into buffers the element tree reuses. Hiding half of 3000 keyed rows
  goes from 18198 to 12192 allocations, and from 12198 to 4693 when the rows are memoized.
