### Changed

- The release profile `flui create` writes links with `lto = "fat"`, builds dependencies at
  `opt-level = "s"` and keeps FLUI's frame crates and the application's own crates at `3`: a
  counter's release binary drops from 27.2 MB to 21.7 MB on Linux x86_64 (wasm32 from 16.7 MB
  to 14.1 MB), and the frame scenarios run no more instructions. `panic` stays `unwind`, since
  FLUI contains a panic in a widget or a callback instead of ending the application.
