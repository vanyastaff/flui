### Changed

- `flui-rendering` keeps per-node state the owner thread alone touches in `Cell`s: `AtomicRenderFlags` is `RenderFlagsCell` (`Cell<RenderFlags>`), the layout generations are `Cell<u64>`, and the parent that placed a node is an `Option<RenderId>` instead of a raw `u64` with `0` for none. `DegradationProbe::new` takes a `&Cell<u64>`.

### Removed

- `flui-rendering` `PipelineOwner::id` and the process-wide counter that minted it: nothing read the id.
