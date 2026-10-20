### Changed

- **A Web wheel scroll and the mouse are one pointer** (`flui-platform`): the mouse keeps one
  contact whatever `pointerId` the engine gives it, with pointer identity one, and a wheel
  scroll carries it; the first touch or pen no longer shares identity one with every wheel
  event.
- **The browser's context menu follows the application** (`flui-platform`, Web): it is
  prevented when the application prevented the default of the secondary press that opened it,
  and shown otherwise; a menu opened by the menu key is still prevented. The FLUI runtime
  reports pointer input handled, so a FLUI app keeps suppressing it.
- **winit moves are delivered once per run** (`flui-platform`): consecutive moves of one
  contact within a loop turn arrive as the last of them, since winit stamps each at dispatch.

### Fixed

- **A reserved mouse split into two live pointers** (`flui-platform`): a reading of the
  reserved contact from another device, or naming none (a Win32 `WM_MOUSEMOVE` after
  `SetCursorPos`, a second mouse), moves the same cursor instead of opening a second contact
  with the same pointer id and its own held buttons.
- **A rejected palm on Android lifted like a finger** (`flui-platform`): an
  `ACTION_POINTER_UP` with `FLAG_CANCELED` is a cancel, so a tap under the palm no longer
  accepts.
- **One winit device id for mouse and touch reported neither present** (`flui-platform`):
  Wayland and X11 give both one id; each kind now gets its own device, so presence lists the
  mouse and the touchscreen.
- **X11's first touch hovered the mouse under the finger** (`flui-platform`, winit): the
  cursor move winit synthesizes for the first touch is dropped, and a touchpad's fractional
  line scroll is reported precise.
- **A Wayland leave mid-press left the button held** (`flui-platform`, winit): the mouse's
  sequence is cancelled at the leave, so the next press after re-entering is a press.
- **A Web press lost to Alt+Tab or a hidden tab swallowed the next press** (`flui-platform`):
  page blur and `visibilitychange` to hidden cancel every open sequence and release its
  capture.
- **Web pens never left** (`flui-platform`): `pointerenter` and `pointerleave` of a mouse or pen
  outside a press deliver `Enter` and `Leave`, and a pen that leaves ends its contact.
- **WebKit on a touch screen scaled a pinch twice** (`flui-platform`): gesture events that start
  while a touch is open are left to the touches.
- **A Control-held mouse wheel at a page zoom zoomed the app** (`flui-platform`, Web): a detent
  that is a whole number of device pixels is a scroll, not a trackpad pinch.
- **Android queried its input device through JNI on every motion event** (`flui-platform`): the
  cache serves its moves, and every other event (a press, a lift, a cancel) queries again.
