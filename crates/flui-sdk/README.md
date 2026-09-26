# flui-sdk

The package-author surface of FLUI: what a package such as a design system
builds on, without the host, the engine or the GPU stack. **Evolving** — it has
its own `0.N` version, bumped on every FLUI release train, and its surface may
change on any train.

Not yet for third-party authors: the first packages to move onto it are FLUI's
own `flui-material` and `flui-cupertino`, and until they build on this crate
alone, the surface is unproven. Applications depend on `flui`, not on this
crate.

## What is in it

- `animation`, `foundation`, `types`, `view`, `widgets`: the same modules the
  `flui` facade exposes, as the same crates, so `flui_sdk::widgets::Text` and
  `flui::widgets::Text` are one type.
- `interaction`, `painting`, `rendering`: the items of the facade's modules
  that packages use, at the same paths.
- `pipeline` (Evolving): render-object internals the facade does not expose.

## What is not

No `flui-app`, `flui-engine` or `wgpu` in its normal dependency graph, which
`cargo xtask reach` checks. The facade neither depends on nor re-exports this
crate. See
[ADR-0088](../../docs/adr/ADR-0088-official-packages-sdk-and-facade.md).
