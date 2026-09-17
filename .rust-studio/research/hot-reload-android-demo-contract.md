# Hot Reload Android Demo Contract Audit

Date: 2026-09-13

Scope: `flui-hot-reload` scene-plugin ownership, Android demo build topology,
and documentation drift around the unsafe `HotReloadDriver::build_scene_or`
contract.

## Evidence

- Read `crates/flui-hot-reload/src/dynlib.rs`, `abi.rs`, `plugin.rs`,
  `worker.rs`, `host.rs`, `driver.rs`, and `dispatch.rs`.
- Read `crates/flui-app/src/app/hot_reload.rs`,
  `examples/android_demo/{Cargo.toml,src/lib.rs}`, `docs/hot-reload.md`,
  and root `Cargo.toml` workspace topology comments.
- Ran `cargo check -p flui-hot-reload --all-targets`: passed.
- Ran `cargo check -p flui-android-demo --target aarch64-linux-android`:
  failed before type-checking because `flui-android-demo` is not a workspace
  package.
- Ran `cargo check --manifest-path examples/android_demo/Cargo.toml --target
  aarch64-linux-android`: failed before type-checking because the package is
  under the root workspace directory but is neither a member nor excluded and
  has no local `[workspace]`.
- `cargo ndk` is not installed in this environment, so no Android NDK build was
  executed.
- Duplicate search for Android demo workspace/hot-reload unsafe API drift found
  no existing flui issue.

## Findings

`ScenePlugin::build_scene` and `HotReloadDriver::{build_scene,build_scene_or}`
correctly expose the dynamic-library lifetime obligation as `unsafe`: a
returned `Scene` may carry plugin-image vtables/drop glue and must be dropped
before the plugin can unload.

The production `flui-app` Android wrapper keeps the driver lock and drops the
scene inside the render block, preserving the documented proof for the current
inline renderer path.

The standalone Android demo is not build-checkable through either the
documented `-p flui-android-demo` shape or a direct `--manifest-path` shape.
That has already allowed drift: `examples/android_demo/src/lib.rs` calls
`hot_reload.build_scene_or(w, h, build_test_scene)` as if it were safe, while
the current API requires an unsafe block and proof.

## Issue

Filed:

- <https://github.com/vanyastaff/flui/issues/1086>
  `hot-reload: make Android scene demo build-checkable and preserve unsafe API contract`

Severity in the issue body: P3 improvement / testability debt with
soundness-contract relevance. GitHub label creation was not available for `P3`,
so the issue was opened without that label.

## Non-findings

- No new issue was filed against `ScenePlugin::build_scene`: the unsafe
  obligation is documented and exposed at the API boundary.
- No new issue was filed against the production `flui-app` Android wrapper:
  its current inline render path keeps the plugin owner alive while the scene is
  rendered and dropped.
- No claim was made that the Android demo fails after type-checking; current
  Cargo topology prevents reaching that evidence.

## Follow-up Leads

- Worker-plugin `BuildPtr` registry still deserves a separate pass for
  reentrant/concurrent registration sessions and stale pointer leases.
- Comments on mature Flutter issues should be mined for solution patterns, but
  each idea must be tested against flui's Rust ownership model rather than
  copied.
