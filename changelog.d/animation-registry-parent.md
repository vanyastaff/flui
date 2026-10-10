### Changed

- Child animation registries have one live parent. `Vsync::attach_child` returns typed refusal for cycles, duplicate or second-parent attachment, closed registries and identity exhaustion.
- `LaidOut::adopt_vsync` returns admission errors while retaining its current driver; adopting the same registry is a no-op.

### Fixed

- Prevent one presentation's motion policy from settling another presentation's shared-child animation, and prevent duplicate child samples within one frame.
- Moving an active registry subtree between presentations preserves its last sample and completion future across independent clock origins. Reentrant withdrawal or ancestor mute stops remaining delivery from the old frame.
- Directly ticking an attached subtree retains its presentation's clock authority and run deadlines.
