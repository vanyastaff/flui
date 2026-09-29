### Fixed

- `flui run --json` forwards the app's last stdout and stderr lines before `run.app.exit`: the
  exit is reported only after both output forwarders reach the end of their streams (bounded at
  two seconds, so a process the app left holding a pipe cannot stall it).
