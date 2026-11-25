### Added

- **`flui-driver`**: `Lane::send_counted` and `lane::Delivered`: a lane counts each command the
  device took of an event, a failing event's own included, so `lane::send_guarded` says what of a
  run went out where one event is none or several commands (an Android key down held for its
  release, text typed around an Enter). The default counts an acting event sent as one command.

### Fixed

- **`flui-driver`**: the Android lane forgets a contact recorded before its press only when the
  device answered by refusing it; a pen report that unwinds ends its device and the record of
  what it held; each dump writes a file of its own, so a file an earlier dump left is never read
  as this one's; every read of the device (a capture included) that times out says nothing may
  have run; a reply naming no focused display is refused where the API level is unread; a
  focused display printed on two lines is refused, and one printed before the window list ends
  is not read.
