### Fixed

- A dropping UI runtime closes its command inbox before it destroys its queued commands and
  frame-failure handler, so a destructor that sends through a kept `UiCommandSender` is refused
  with `OwnerGone` instead of queueing a capture that would abort the process.
