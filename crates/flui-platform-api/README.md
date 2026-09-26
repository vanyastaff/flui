# flui-platform-api

Platform **contracts** for FLUI: the capability traits and the window, input
and data-transfer vocabulary, with no OS backend behind them.

## What is in it

- Capability traits: `PlatformTextInput` (IME), `PlatformHaptics`,
  `PlatformDisplay`, `Clipboard`, and the data-transfer transport
  (`data_transfer::DataTransferSource`, ADR-0038).
- Input vocabulary: `PlatformInput`, `DispatchEventResult`, `DragDropEvent`,
  the pointer and keyboard types, and the pixel conversion helpers.
- The per-window contract `PlatformWindow` and the window vocabulary:
  `WindowId`, `WindowOptions`, `WindowReveal`, `WindowMode`, `WindowEvent`,
  `WindowExecutionState`, `CursorIcon` and the errors window operations return.

## What is not

No OS, winit, AccessKit or tokio type appears here, and the crate has no
`unsafe`. The OS backends, the host-facing `Platform` trait and `HostWindow`
(a `PlatformWindow` plus its AccessKit accessibility bridge) live in
`flui-platform`, which re-exports everything in this crate at its old paths.
Only composition roots (`flui-app`) depend on `flui-platform`.

## Who depends on this

A crate or plugin that programs against a platform capability, without linking
the backends that implement it:

```toml
[dependencies]
flui-platform-api = "0.2"
```

```rust,ignore
use std::sync::Arc;
use flui_platform_api::PlatformTextInput;

fn focus_text_field(input: &Arc<dyn PlatformTextInput>) {
    input.set_ime_allowed(true);
}
```

Inside the workspace, framework crates that name a capability depend on this
crate; only composition roots depend on `flui-platform`. See
[ADR-0082](../../docs/adr/ADR-0082-platform-api-contract-crate.md).
