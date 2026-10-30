### Fixed

- **iOS haptics:** a Core Haptics restart that completes after the engine stopped again no longer
  reports the engine running. The engine's state carries the epoch of its last stop, so the stop
  stays owed and the engine is restarted again; a stop reported while the engine first starts is
  kept the same way. Type-checked only; not yet run on a device.
