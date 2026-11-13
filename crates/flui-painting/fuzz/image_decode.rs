//! The fuzz target of `Image::decode`: `cargo xtask fuzz image_decode` builds
//! it under `--cfg fuzzing` and libFuzzer mutates encoded images toward code
//! no input has reached yet, starting from `tests/fixtures/`. A panic, an
//! abort, a read out of bounds, an allocation past the limit or a decode past
//! the time limit ends the run and leaves the input that did it;
//! `cargo xtask fuzz image_decode --replay <file>` runs that input again in
//! the same build.
//!
//! Built without that flag it decodes each file named on its command line
//! with the decoders an application ships. Those check a PNG's checksums,
//! which a fuzz build skips, so an input the fuzzer found may be refused
//! here before it reaches what it found.

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
