### Fixed

- **`flui-native`**: a window of the native Wayland host is no longer reported occluded between
  its first configure and its first frame. A surface enters an output only once the compositor
  maps it with a frame, so an application that held frames back from the occluded window never
  drew the frame that maps it; the window is now occluded only before its configure, while
  suspended, or once shown and on no output.
