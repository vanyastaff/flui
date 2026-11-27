### Added

- The facade's `webgl2` feature (and `flui-app`'s and `flui-engine`'s): a web build tries WebGL2
  when the browser gives no WebGPU adapter. `flui create` names it in every application it
  generates, with a comment that removing it ships the web build on WebGPU only, about 2 MB
  smaller (ADR-0404).

### Changed

- A web build that does not name `webgl2` carries no GL backend and refuses a browser without
  WebGPU at start, naming WebGPU as the backend tried. An application whose manifest
  `flui create` did not write adds `features = ["webgl2"]` to its `flui` dependency to keep the
  WebGL2 fallback (ADR-0404).
