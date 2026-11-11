### Changed

- A UI runtime with one presentation enters its `GlobalKey` scope without assembling a composite registry: a label change allocates 15% less per frame.
- A pipeline owner run in place leaves a placeholder that shares its handles instead of building a fresh owner: a frame no longer builds a dirty channel, notifier, inbox and seal twice, nor mints two pipeline ids.
- `TextPainter::layout` shapes its text once: the intrinsic widths it caches come from the layout's own shaping unless an ellipsis replaced the text.
