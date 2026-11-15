### Changed

- Wayland: reads of a selection another client owns, the clipboard's and the primary one, share one read per offer. A read made while one is under way gets its answer, and past four reads waiting a read resolves at once as `ClipboardError::Unavailable`, as on X11. Each read used to start a thread of its own that waited up to five seconds on the owner.

### Fixed

- Wayland: the frame a window draws itself is drawn at the window's scale up to 8 and never with more than 18.5 MiB of pixels, into at most three buffers a surface that it draws into again once the compositor releases them. A compositor naming a scale in the hundreds had a narrow window's title bar take a gibibyte at every hover, and every redraw made a new memory file.
- Wayland: with several seats, one seat's pointer leaving the window's frame no longer lets go of the button another holds, and a press on the frame is answered where its own pointer is.
- Wayland: a seat the compositor removes is forgotten. The keys its keyboard held are released in their window, no drag starts under the press it held, and no activation names it.
- Wayland: a compositor that introduces data offers it never names as a selection or a drag no longer has the client hold each of them: a device keeps the last and destroys the others.
- Wayland: a compositor whose `wl_compositor` is older than version 4 is no longer sent `wl_surface.damage_buffer` for a window's own frame, a request that version lacks.
- Wayland: a drag target asking for a session's data faster than it reads it no longer has the application's provider run for the requests that are refused.
- Wayland: the host keeps no list of the globals a compositor announces, and binds `wl_compositor` and `wl_shm` once instead of once for the windows and once for cursors.
