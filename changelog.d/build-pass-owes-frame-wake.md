### Fixed

- **A failing frame request no longer resets widgets** (`flui-view`): a build pass that
  scheduled a rebuild (an inherited provider notifying its dependents, for one) ran the
  frame-request hook inline, so a hook that panicked unwound through that element's update
  and the subtree was replaced by an error view; on the next frame it mounted again with
  fresh state. A `SelectionArea` whose long press ran in such a frame lost its selection,
  so a tap or Esc had nothing to clear. The pass now requests the frame once it is over;
  the failure still reaches the caller and the request is retried.
