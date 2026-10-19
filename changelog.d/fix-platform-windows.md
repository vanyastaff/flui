### Fixed

- A Windows window's pen haptics, drag source and drop target reach only the window they were
  made for, never a later window the OS built behind a recycled handle.
- A Windows OS drag accepted but not yet started is told `Cancelled` when the platform's owner
  window goes; a platform dropped by a panic runs none of the session's code, and a session
  whose end hook panics keeps its provider rather than releasing it.
- A Windows window whose DirectManipulation viewport fails stops reporting precise scrolling
  and trackpad gestures.
- The modal OLE drag loop delivers a replaced wake-deadline hook's deadline even when it
  answers the instant its predecessor was delivered at.
- Panics in Windows COM callbacks (OLE drag and drop, DirectManipulation) are logged with
  their message, and their payloads are retained rather than dropped inside the OS's call.
- A Windows drag over a window whose position cannot be converted is no longer reported at
  the window's origin, and a touchpad manipulation is no longer placed at the origin when the
  cursor has never been read (the manipulation is dropped instead of pumped indefinitely).
- Android semantic haptics no longer block the owner thread for up to 8 ms waiting on the Java
  main thread: one call is posted at a time, a request made while it waits reports
  `Suppressed`, and a declining view reports `Suppressed` until a call plays again.
- A Windows TSF document reads its window's scale and origin only while its handle still names
  that window.
