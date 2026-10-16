### Fixed

- Allow heli-scheduled xtask commands to progress while another checkout holds
  the standalone host lock. Managed commands inherit heli's scheduling instead
  of joining a second machine-wide queue; standalone commands retain the lock.
- Preserve the running Windows xtask executable in mixed-package scoped test
  plans by testing the driver separately from feature-unified consumer packages.
- Keep Messenger subprocess readiness markers in temporary storage so tests
  also work when Cargo's target and build directories are outside the checkout.
