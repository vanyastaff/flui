### Fixed

- An X11 window answers its window manager's ping (`_NET_WM_PING`) as the application's loop
  turns, so GNOME, KDE and Xfce can offer to end an application that stopped responding, as they
  already could on Wayland. The window names the machine it runs on (`WM_CLIENT_MACHINE`), and its
  process id beside it only outside a sandbox's or container's PID namespace, where the id names
  this process for the window manager too.
- An interactive resize under an X11 window manager that synchronises resizes
  (`_NET_WM_SYNC_REQUEST`: mutter, KWin, xfwm4, openbox) waits for the window to draw each new
  size before it takes the next step, instead of resizing faster than the window repaints. Under a window
  manager that synchronises frames (`_NET_WM_FRAME_DRAWN`), the window tells the compositor while
  it draws each frame.
