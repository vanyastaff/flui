### Fixed

- **`flui-native`**: on the X11 host, a drag whose source application crashes or is killed while
  it is over a window now ends: the window hears `DragDropEvent::Exited` and its offer is stale,
  where before it kept hovering, with the offer live, until the next drag entered. A drop the
  window took from such a source answers its requests `TransferError::SourceGone` at once instead
  of timing out.
- **`flui-native`**: on the X11 host, a client that goes in the middle of reading a large clipboard or
  primary selection no longer holds one of the transfers the clipboard writes at once until it times out:
  the next client is served as soon as its window is gone.
