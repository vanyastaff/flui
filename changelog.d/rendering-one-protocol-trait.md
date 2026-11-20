### Removed

- `flui_rendering::protocol::{LayoutCapability, HitTestCapability}` and the marker types
  `BoxLayout`, `BoxHitTest`, `SliverLayout` and `SliverHitTest` (also gone from the crate root
  and from `flui::rendering`): a protocol's types are associated types of `Protocol` itself.
  Write `P::Constraints` for `<P::Layout as LayoutCapability>::Constraints`,
  `P::LayoutContext<'ctx, A, PD>` for `<P::Layout as LayoutCapability>::Context<'ctx, A, PD>`,
  `P::HitTestContext<'ctx, A, PD>` for `<P::HitTest as HitTestCapability>::Context<'ctx, A, PD>`,
  and `P::HitResult` / `P::HitEntry` for the capability's `Result` / `Entry`.
- The aliases `ProtocolConstraints<P>`, `ProtocolGeometry<P>`, `ProtocolPosition<P>` and
  `ProtocolHitResult<P>`: write `P::Constraints`, `P::Geometry`, `P::Position` and
  `P::HitResult`, or the concrete type (`BoxConstraints`, `Size`, `Offset`, `SliverGeometry`,
  `MainAxisPosition`, …) where the protocol is known.
- `flui_rendering::protocol::prelude`: import from `flui_rendering::protocol` by name.
- The public modules `flui_rendering::protocol::box_protocol` and
  `flui_rendering::protocol::sliver_protocol`: every public item they held is at
  `flui_rendering::protocol::…` (for example `protocol::LayoutChildCallback`,
  `protocol::ErasedBoxLayoutContext`). The two `BoxChildIntrinsicCallback` aliases are one,
  `protocol::BoxChildIntrinsicCallback`.

### Changed

- `Protocol` carries `Constraints`, `Geometry`, `CacheKey`, `LayoutContext`, `Position`,
  `HitResult`, `HitEntry` and `HitTestContext` as its own associated types, and `cache_key` and
  `normalize_constraints` beside `default_geometry` and `validate_constraints`.
- `LayoutContextApi<'ctx, P, A, PD>` and `HitTestContextApi<'ctx, P, A, PD>` take the protocol
  (`P: Protocol`) as their first type parameter: write `LayoutContextApi<'ctx, BoxProtocol, A, PD>`
  for `LayoutContextApi<'ctx, BoxLayout, A, PD>`.
- `Protocol` and its associated types no longer require `Send + Sync`: a protocol's values live
  on the thread of the pipeline owner that lays them out.
