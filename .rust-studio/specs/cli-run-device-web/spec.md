# `flui run --device browser:<name>`

Branch `cli/run-device-web` (worktree `flui-cli-wt`, from `main` after v0.1.0).

## Problem

`flui run --device browser:chrome` exits 2 ("no browser target yet"). Worse,
`flui build web` cannot build a generated project at all: `build/web.rs` runs
`wasm-pack` inside `<root>/crates/flui_app`, a layout no template produces,
and the templates are `fn main` binaries, not `cdylib`s. Nothing tests it.

## Design

### Web build (`build/web.rs`)

Trunk's model, not wasm-pack's: a `fn main` binary compiles for
`wasm32-unknown-unknown` unchanged (`run_app` dispatches to the web runner on
wasm32; `examples/web_counter` is the evidence), and `wasm-bindgen --target
web` turns the `.wasm` into `pkg/app.js` + `pkg/app_bg.wasm`, the pair the
web template's `index.html` already imports.

1. `validate_environment`: `wasm32-unknown-unknown` installed; `wasm-bindgen`
   on PATH; its `--version` equals the project's resolved `wasm-bindgen`
   crate (`cargo metadata --filter-platform wasm32-unknown-unknown`) — the CLI
   refuses to start otherwise, because the mismatch error wasm-bindgen prints
   later is cryptic. Hint: `cargo install wasm-bindgen-cli --version <v> --locked`.
2. `build_rust`: `cargo::select_target` (the project's bin), then
   `cargo::build_artifact` with `--target wasm32-unknown-unknown` and the
   profile flag; returns the `.wasm`.
3. `build_platform`: `wasm-bindgen --target web --no-typescript --out-dir
   <dist>/pkg --out-name app <wasm>`, then `index.html`, `manifest.json`,
   `icons/` from `platforms/web/` into `<dist>`. `<dist>` is the context's
   `output_dir`. No `platforms/web/dist` intermediate any more.

### Dev server (`serve.rs`, std only)

`DevServer::start(root, port)`: `127.0.0.1:port` (0 = ephemeral), one accept
thread, one thread per connection, `Connection: close`. GET/HEAD only.
Path: query stripped, must not contain `..`; `/` is `index.html`. MIME by
extension (html js mjs wasm json css png svg ico txt map woff2), everything
else `application/octet-stream`. `Cache-Control: no-store`.

Live reload without a websocket: `index.html` is served with
`<script src="/__flui/reload.js"></script>` injected before `</body>`; the
script long-polls `/__flui/reload?since=<generation>`, which answers when the
server's generation moves past `since` (or after 25 s with the current one),
and calls `location.reload()`. `DevServer::reload()` bumps the generation.
`Drop` stops the accept loop (flag + self-connect) and joins it.

### Run (`commands/run.rs`)

`Target::Browser { id, name, launch }` from `flui devices` details
(`path`: macOS `.app` bundle, Linux command, Windows executable). A new
`ReloadStrategy`, `BrowserSession`: `build_and_spawn` = web build → start the
server once (`run.web.serve {url, dir}`) → open the browser once → on later
builds `server.reload()` (`run.reload {kind: "page", ok}`). `watch_paths`:
`src/`, `Cargo.toml`, `platforms/web/`. No child process: the trait gains
`is_running(&self, child)` (default `child.is_some()`), which the loop uses
for its "build failed" messages instead of peeking at the child.

New flags: `--web-port <PORT>` (default 0) and `--no-open`.
`--no-hot-reload` with a browser: build, serve, open, wait for `q`/Ctrl-C
(no watching).

Opening: macOS `open -a <bundle> <url>`; Linux/Windows `<path> <url>`
detached, stdio null.

### Doctor

`web.tooling` looks for `wasm-bindgen` (hint `cargo install wasm-bindgen-cli`);
wasm-pack is no longer mentioned.

## Tests

- `serve.rs` unit: MIME, 404, traversal rejected, HEAD, index injection,
  long-poll returns on `reload()`.
- `build/web.rs` unit: version comparison and the install hint.
- `run.rs` unit: browser launch command per OS; `Target` labels/ids.
- `tests/cli_run.rs`: `--device browser:nonexistent` → exit 5.
- Live (`FLUI_CLI_LIVE_WEB=1`): `flui create --local` + `flui build web
  --json` produces `pkg/app.js` and `pkg/app_bg.wasm`; `flui run --device
  browser:<installed> --no-open --json` emits `run.web.serve`, the URL serves
  `index.html` with the reload script, SIGINT ends it with `run.stop`.

## Out of scope

Android `run --device`, Windows hot-keys, HTTPS, a bundler.
