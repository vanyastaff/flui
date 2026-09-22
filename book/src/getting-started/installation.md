# Installation and first run

This page covers the app-author path: install the toolchain, generate an application with
`flui create`, and run it. For the fuller reference (full prerequisites table, every bundled
example, troubleshooting) see
[`docs/getting-started.md`](https://github.com/vanyastaff/flui/blob/main/docs/getting-started.md)
in the repository — this page is the short version, copied verbatim from the commands there so it
never invents a step the full doc doesn't have.

If you want to work on FLUI itself rather than build an app with it, see
[Contributing to FLUI](contributing.md) instead — the setup is different (a workspace checkout,
not a generated application).

## Prerequisites

| Tool | Minimum version | Notes |
|------|-----------------|-------|
| Rust | 1.97 | MSRV floor in `workspace.package.rust-version`; `rustup` installs the pinned dev toolchain automatically on first `cargo` invocation. |
| Native toolchain | platform-specific | MSVC on Windows, Xcode CLT on macOS, NDK on Android (only if targeting Android). |

FLUI is not yet published to crates.io — the beta itself will not be published there until a beta
tag is cut. Until then, `flui create` generates an application against a local checkout of this
repository.

## Install the CLI and create an app

Run these commands from a checkout of the [flui repository](https://github.com/vanyastaff/flui):

```bash
cargo install --path crates/flui-cli --locked
flui create my_app --local --path ../apps
cd ../apps/my_app
flui run
```

The generated application can live outside the FLUI repository. Bare `--local` uses the current
directory as its source checkout; from another directory, use `--local=/path/to/flui`. The source
must remain available afterward — the generated `flui` dependency points at the checkout root by
absolute path. `flui` is the application's only framework dependency; UI code starts with
`use flui::prelude::*;`.

Add `--hot-reload` to `flui create` to generate the host/worker/types workspace used by the reload
runner — see the
[CLI guide](https://github.com/vanyastaff/flui/blob/main/crates/flui-cli/README.md) for template
and build options.

## Run a bundled example instead

To try FLUI without generating a project first, run one of the bundled examples from the
repository checkout:

```bash
cargo run --example counter
```

A window opens showing a count and an "Increment" button — the same shape `flui create`'s
`counter` template generates. See the full example table and web/Android instructions in
[`docs/getting-started.md`](https://github.com/vanyastaff/flui/blob/main/docs/getting-started.md#run-an-example).

## Next steps

- [Concepts](../concepts/overview.md) — the mental model behind `View`/`Element`/`RenderObject`.
- [Flutter → FLUI mapping](../mapping.md) — if you already know Flutter's vocabulary.
- [Cookbook](../cookbook/overview.md) — task-oriented examples.
