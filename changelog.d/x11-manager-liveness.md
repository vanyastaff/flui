### Fixed

- An X11 window answers its window manager's ping (`_NET_WM_PING`) as the application's loop
  turns, so GNOME, KDE and Xfce can offer to end an application that stopped responding, as they
  already could on Wayland. The window names the machine it runs on (`WM_CLIENT_MACHINE`) beside
  its process id, as EWMH requires.
- An interactive resize under an X11 window manager that synchronises resizes
  (`_NET_WM_SYNC_REQUEST`: mutter, KWin, xfwm4, openbox) waits for the window to draw each new
  size, instead of drawing the window's frame around content of the old size. Under a window
  manager that synchronises frames (`_NET_WM_FRAME_DRAWN`), the window tells the compositor while
  it draws each frame.
