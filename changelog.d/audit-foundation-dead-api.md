### Removed
- `flui-foundation` drops the build and platform constants `DEBUG_MODE`, `RELEASE_MODE`, `IS_WEB`, `IS_MOBILE` and `IS_DESKTOP`, and the single-precision `EPSILON_F32`, from `consts`, the crate root and the prelude. Nothing in the workspace used them; use `cfg!(debug_assertions)`, `cfg!(target_family = "wasm")` and `cfg!(target_os = ...)` directly. `EPSILON` stays.
