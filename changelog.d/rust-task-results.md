### Changed
- Ready platform tasks can return borrowed, non-`Send`, and non-`Unpin` results; executor spawning retains its thread and lifetime requirements.
