//! The fuzz target of `Image::decode`: `cargo xtask fuzz image_decode` builds
//! it under `--cfg fuzzing` and libFuzzer mutates encoded images toward code
//! no input has reached yet, starting from `tests/fixtures/`. A panic, an
//! abort, a read out of bounds or a decode past the time limit ends the run
//! and leaves the input that did it.
//!
//! Built without that flag it decodes each file named on its command line,
//! which is how a crash input is replayed on the pinned toolchain:
//! `cargo run -p flui-painting --example fuzz_image_decode -- <file>`.

#![cfg_attr(fuzzing, no_main)]

#[path = "../tests/support/foreign_image.rs"]
mod foreign_image;

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    foreign_image::decodes_within_the_limit_or_fails_typed(bytes);
});

#[cfg(not(fuzzing))]
fn main() -> std::io::Result<()> {
    for path in std::env::args_os().skip(1) {
        foreign_image::decodes_within_the_limit_or_fails_typed(&std::fs::read(path)?);
    }
    Ok(())
}
