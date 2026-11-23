### Changed

- **`flui-cli`**: a project `flui create` writes declares no `cdylib`: the library is an rlib, and
  `flui build android` and `flui run` on an Android device compile it as the `cdylib`
  `NativeActivity` loads with `cargo ndk rustc --lib --crate-type cdylib`. A desktop debug build
  no longer links a second copy of the framework into a shared library on every edit, which took
  0.6–0.8 s of the 2.1 s from a one-line edit to a built counter. A project that still declares
  the `cdylib` builds as before.
