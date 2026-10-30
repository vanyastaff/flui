### Fixed

- A window closed while a field composes commits the composition, keeping its text, before the
  window's Detached observers run, whichever way the window closes (accepted close, the
  platform destroying it, the session's end): what they save holds the preedit as text.
