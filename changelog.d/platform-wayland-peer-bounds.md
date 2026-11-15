### Changed

- Wayland: reads of a selection another client owns, the clipboard's and the primary one, share one read per offer. A read made while one is under way gets its answer, and once a few reads wait already, as many as the X11 host lets wait, the next read resolves at once as `ClipboardError::Unavailable`. Each read used to start a thread of its own that waited up to five seconds on the owner.

### Fixed

- Wayland: a window's content and the frame it draws itself are drawn at no more than eight device pixels per logical pixel, whatever fractional, preferred or output scale the compositor names, and a window the compositor makes larger than the renderer's largest surface at that scale is drawn at a lower one that fits instead of waiting, unrendered, for another size. Its own frame is drawn at the same scale, into a few buffers a surface which it draws into again once the compositor releases them. A compositor naming a scale in the hundreds had a narrow window's title bar take a gibibyte at every hover, and every redraw made a new memory file.
- Wayland: with several seats, one seat's pointer leaving the window's frame no longer lets go of the button another holds, and a press on the frame is answered where its own pointer is.
- Wayland: a seat the compositor removes is forgotten. The keys its keyboard held are released in their window, no activation names it, and a drag session started under its press ends as cancelled instead of leaving every later drag refused as busy; so does one whose seat loses its pointer with the button held. A close, minimize or maximize asked on the window's own frame is still carried out when the seat goes in the same moment.
- Wayland: a compositor that introduces data offers it never names as a selection or a drag no longer has the client hold each of them: a device keeps the last and destroys the others.
- Wayland: a compositor whose `wl_compositor` is older than version 4 is no longer sent `wl_surface.damage_buffer` for a window's own frame, a request that version lacks.
- Wayland: a drag target asking for a session's data faster than it reads it no longer has the application's provider run for the requests that are refused.
- Wayland: the host keeps no list of the globals a compositor announces, binds none announced at version zero, and binds `wl_compositor` and `wl_shm` once instead of once for the windows and once for cursors.
