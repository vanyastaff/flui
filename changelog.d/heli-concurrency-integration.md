### Fixed

- Allow heli-scheduled xtask commands to progress while another checkout holds
  the standalone host lock. Managed commands inherit heli's scheduling instead
  of joining a second machine-wide queue; standalone commands retain the lock.
