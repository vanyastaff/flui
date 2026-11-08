### Fixed

- **`flui-native`**: clipboard reads on Android, macOS, iOS and the web now measure the exact
  decoded UTF-8 length before allocating the Rust `String`, and refuse text past 64 MiB without
  that allocation. Android follows JNI's modified-UTF-8 conversion, including its lossy fallback;
  Windows and the web share the same bounded UTF-16 length decoder.
- **`flui-native`**: all five HTTP providers now enforce the same 1024-response-header limit and
  share `ByteString` decoding and the declared `Content-Length` preflight, instead of applying
  those policies only on selected hosts.
- **`flui-native`**: Android reports TalkBack cursor transitions through a queued push handler
  outside the adapter lock, before JNI events can re-enter it. Removing a focused node clears its
  stored ID, so re-adding that ID cannot revive stale focus.
- **`flui-native` / `flui-platform`**: AppKit, UIKit, Wayland and Win32 Direct Manipulation now drop
  non-finite scroll deltas instead of delivering substitute zero events. Rejected deltas no longer
  open or extend scroll sequences; finite zero deltas and terminal phases remain intact.
