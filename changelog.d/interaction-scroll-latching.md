### Fixed
- Keep each scroll gesture with its first consumptive target, including at nested scroll extents and across focal-point motion; fresh pointer observers still receive every packet. Release selection on terminal phases, source or presentation cancellation, or 500 ms of phase-less wheel inactivity.
- Keep absent-device source identity stable when pointer tool or role metadata changes.
