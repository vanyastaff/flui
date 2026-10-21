### Fixed

- A wake raised after the owner loop started, while no owner-turn callback was registered
  (`OwnerPlatform::on_wake`), was lost; it now runs the next callback registered, once.
