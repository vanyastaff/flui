### Added

- **`flui-driver`**: `Lane::send_counted` and `lane::Delivered`, the one record
  `lane::send_guarded`'s partly sent answer is read from: a lane says each input of an event that
  reached the device, and the run counts every event taken whole and the failing event where part
  of it went out.

### Fixed

- **`flui-driver`**: the Android lane forgets a contact recorded before its press only when the
  press never reached the device or the device refused it; a pen report that unwinds ends its
  device and the record of what it held; each dump writes a file of its own, so a file an earlier
  dump left is never read as this one's; every read of the device (a capture included) that
  times out says nothing may have run; a reply naming no focused display is refused where the API
  level is unread; a focused display printed on two lines is refused, and one printed before the
  window list ends is not read.
