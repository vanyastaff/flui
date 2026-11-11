### Added
- `flui_layer::read_closure` and `close_over`: one answer to what an effect reads and writes (`Closure::{None, Bounded, Unbounded}`), and the damage grown over every closure it meets to a fixed point.
- `SceneOwner::patch_damage`: the damage of the last applied patch, the footprint of its operations under the versions before and after it, with followers resolved again in both.

### Changed
- `LayerDiffer` and the engine's backdrop planning close damage over `read_closure`: a backdrop filter's damage covers what it samples, and a colour filter that paints transparent black makes the frame `Full(Unbounded)`.
