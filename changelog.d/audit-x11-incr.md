### Added

- The native X11 host's XDND drag source writes a payload past one property (256 KiB, or less
  under the server's request limit) in `INCR` chunks instead of refusing the conversion. At most
  four such answers are written at once, holding at most 64 MiB of payload between them (answers of
  the same bytes share one copy); one whose requestor takes no chunk for five seconds, or
  whose window is destroyed, is let go of, and the session's end ends them all.
