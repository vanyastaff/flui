# flui-log

Cross-platform logging **backend** for FLUI — the crate that assembles a
`tracing` subscriber, not the crate framework code logs through.

## Who depends on this

Composition roots only: `flui-app`, `flui-cli`, and the `flui` facade.

Framework libraries (`flui-view`, `flui-rendering`, `flui-widgets`,
`flui-engine`, …) depend on `tracing` and nothing else. They emit structured
events and have no opinion about where those events go, so the default backend
can be replaced or removed without touching a single instrumentation call site.
`docs/workspace-layers.toml` enforces the rule mechanically — a framework crate
that adds a normal dependency on `flui-log` fails `just inventory-check`.

## Ownership, not configuration

Installing a subscriber writes process-global state exactly once, and FLUI may
be embedded in a process whose observability somebody else already owns. So the
question gets an explicit answer instead of a panic:

| Policy | Effect |
|---|---|
| `Inherit` | Installs nothing, reads nothing, changes nothing |
| `Auto` | Installs the platform default *only* if the slot is empty; an existing subscriber is preserved |
| `Install` | Demands the slot, and returns a typed error if it is taken |

Each returns a `SubscriberInstallation`: `ownership` is `Installed` or
`Unchanged`, and `log_bridge` reports whether the compatibility bridge for
dependencies using the `log` facade was installed or an existing logger was
preserved.

The two global slots have separate policies. `LogBridgePolicy::Auto` installs
`LogTracer` when possible; `LogBridgePolicy::Inherit` leaves the `log` facade
untouched for a host that owns it independently of tracing.

```rust,no_run
use flui_log::{LogConfig, SubscriberPolicy};

let installation = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)?;
# Ok::<(), flui_log::SetupError>(())
```

## Composing your own stack

`setup` is the convenience path. A tool that needs its own layers builds the
pieces and installs the result itself:

```rust,no_run
use flui_log::{InstallPolicy, LogBridgePolicy, LogConfig, PlatformLayer, install_subscriber};
use tracing_subscriber::{Registry, layer::SubscriberExt as _, Layer as _};

let config = LogConfig::builder().directives("info,flui_view=trace").build();
let filter = config.env_filter()?;

let subscriber = Registry::default()
    .with(PlatformLayer::platform_default(&config).with_filter(filter));
//  .with(my_timeline_layer)   <- devtools stacks here

install_subscriber(subscriber, InstallPolicy::Install, LogBridgePolicy::Auto)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Platforms

| Target | Backend | Viewer |
|---|---|---|
| Desktop, incl. macOS | `tracing_subscriber::fmt`, or `tracing-forest` with the `hierarchical` feature | terminal |
| Android | logcat via `__android_log_write` | `adb logcat` |
| iOS | Apple unified logging | Console.app, Xcode, `log stream` |
| wasm32 | browser console + performance timeline | DevTools |

macOS keeps the desktop backend on purpose: unified logging there would make
`cargo run` print nothing. A bundled macOS app enables the
`apple-unified-logging` feature and asks for `PlatformLayer::apple_unified_logging`.

Bridged `log` records are normalized before Android chooses a logcat tag, so a
record from `wgpu` remains filterable and groupable as `wgpu` rather than
appearing under the synthetic `log` target. Apple unified logging does not map
Rust targets to categories at all: the application subsystem and the stable
`flui` category are fixed for the sink, while `log.target` remains part of the
rendered fields.

### Apple privacy

`tracing-oslog` renders each event into one already-formatted string, so every
field FLUI emits is **public** in the unified log — `%{private}` redaction
applies to interpolated arguments and there are none. Keep secrets and personal
data out of tracing fields on Apple platforms.

This is a convention today, not a contract enforced by the type system. Making
it one — private by default, backend-redacted — is tracked in
[#572](https://github.com/vanyastaff/flui/issues/572).

## Filtering

`RUST_LOG` (or a configured variable) wins over the built-in directives. Its
filter is attached to the native sink rather than globally to the registry, so
timeline, metrics, and capture layers can choose independent filters. Within
the native sink nothing narrows the result afterwards:
`RUST_LOG=flui_view=trace` really does deliver `TRACE`.

## Features

| Feature | Effect |
|---|---|
| `hierarchical` | Adds `DesktopFormat::Hierarchical`, a `tracing-forest` span tree printed when each root span closes |
| `apple-unified-logging` | Makes `os_log` selectable on macOS (always available on iOS) |

## Out of scope

Crash persistence, remote upload, and the user-visible error surface are
separate services. This crate may feed them; it does not own their lifecycle or
product policy.
