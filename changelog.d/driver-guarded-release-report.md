### Fixed

- A guarded OS-input sequence whose send failed (interrupted by the user, the window gone) no
  longer drops a failed release after it: the release is tried once more, and what stays held is
  logged at error level, while the call still answers the send's error.
