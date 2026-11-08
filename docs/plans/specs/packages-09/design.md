# Design: example, template and acceptance parity

## Counter source

`crates/flui-cli/src/templates/counter_app.rs` is the sole counter application source. The generator uses `include_str!("counter_app.rs")`, keeping the file inside the packaged CLI crate. `examples/counter.rs` and `tests/agent_workflow.rs` use `include!` of that exact file; the acceptance places it in a module and mounts its public `CounterApp`.

The source keeps the generated `SafeArea`/`RawButton` tree and template tests. Acceptance adds only the production media, gesture and focus scopes required to mount it, then performs its existing facade-only render/semantics/replay assertions. Material-specific structure assertions are removed with the Material button.

## Hot reload

The hand-written `examples/hot_reload_counter` is deleted. The CLI hot-reload template remains the only source of the host/worker/types project. `cli_create` generates a local project and runs offline `cargo check --workspace`, proving the complete generated graph compiles. The reach fact requires `flui-hot-reload/app-plugin` through the facade's real `hot-reload` feature root.

## Example ownership

Engine scene/render and painting fixtures move under `crates/flui-engine/examples`; the native web demo moves under `crates/flui-native/examples`; the reload lifecycle fixture moves under `crates/flui-hot-reload/examples`. Workspace members and every live path reference follow the move.

Nested example crates are classified as application crates by workspace metadata checks. Example-only dependencies remain development dependencies; any exception names the governing ADR and does not change production edges.

Material and Cupertino gain small headless executables under their own `examples/`. They use only their package and `flui-sdk`, mount real controls with `flui_sdk::testing::widgets::lay_out`, resolve current theme data and print observed results. They make no native-window, GPU or browser support claim.

## Records and validation

Update example and hot-reload instructions in place, remove the deleted counter from live naming records and add a changelog fragment. Validate source parity with a deliberate counter mutation, generated-project compilation, moved-target checks, package-example execution, workspace/reach/docs gates, strict Clippy, formatting and diff checks.
