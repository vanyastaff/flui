### Fixed

- Preserve accepted scroll commands and subsequent motion after a custom curve
  destructor or position listener panics. Retire displaced commands outside
  owner-local storage and preserve newer commands created during retirement
  or by a running driver's cancellation observers.
