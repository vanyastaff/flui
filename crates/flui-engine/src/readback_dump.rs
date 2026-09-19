//! Readback frame dumps for GPU oracle tests.
//!
//! When the `FLUI_READBACK_DUMP_DIR` environment variable is set, every
//! readback helper in the `wgpu` test modules dumps the frame it just read
//! back into that directory as `<test-name>.png` (RGBA8). The GPU CI job
//! sets the variable and uploads the directory as an artifact when an oracle
//! test fails, which is the only way to inspect the actual frame produced on
//! a software rasterizer that cannot be reproduced locally. With the
//! variable unset every entry point is a no-op, so dumps cost nothing in
//! normal runs.
//!
//! Dump failures (dir creation, encoding, I/O) are logged via
//! `tracing::warn!` and swallowed: this is best-effort diagnostics and must
//! never fail a test.

use std::path::{Path, PathBuf};

/// Environment variable that enables frame dumps when set.
///
/// Read only; never written by this crate. `std::env::set_var` is unsound in
/// a multi-threaded process (libc `getenv` on another thread — the Vulkan
/// loader reads `VK_*` while an adapter is requested), so the tests below
/// pass the directory explicitly instead of mutating the environment.
#[cfg(feature = "testing")]
const DUMP_DIR_ENV: &str = "FLUI_READBACK_DUMP_DIR";

/// Dump `rgba` (RGBA8, `width`×`height`) as `<dump-dir>/<test-name>.png`,
/// deriving the test name from the current thread — in test threads that is
/// the test name under both `cargo test` and nextest, so call sites in
/// readback helpers stay one-liners.
///
/// No-op unless `FLUI_READBACK_DUMP_DIR` is set. Never panics.
#[cfg(feature = "testing")]
pub(crate) fn dump_frame(width: u32, height: u32, rgba: &[u8]) {
    let thread = std::thread::current();
    let test_name = thread.name().unwrap_or("flui_readback");
    dump_rgba_png(test_name, width, height, rgba);
}

/// Dump `rgba` (RGBA8, `width`×`height`) as `<dump-dir>/<test_name>.png`.
///
/// `test_name` is sanitized into a safe file name: `::`, path separators,
/// and any other character outside `[A-Za-z0-9._-]` become `_`. No-op unless
/// `FLUI_READBACK_DUMP_DIR` is set. Never panics; write failures are logged
/// and swallowed.
#[cfg(feature = "testing")]
pub(crate) fn dump_rgba_png(test_name: &str, width: u32, height: u32, rgba: &[u8]) {
    let dir = std::env::var_os(DUMP_DIR_ENV).map(PathBuf::from);
    dump_rgba_png_to(dir.as_deref(), test_name, width, height, rgba);
}

/// [`dump_rgba_png`] with the destination given explicitly: `None` is the
/// "variable unset" no-op.
fn dump_rgba_png_to(dir: Option<&Path>, test_name: &str, width: u32, height: u32, rgba: &[u8]) {
    let Some(dir) = dir else { return };
    if let Err(err) = write_png(dir, test_name, width, height, rgba) {
        tracing::warn!(
            dir = %dir.display(),
            test_name,
            "readback dump failed: {err}"
        );
    }
}

/// Encode `rgba` as a PNG under `dir`; the caller logs any error.
fn write_png(
    dir: &Path,
    test_name: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let path = dir.join(format!("{}.png", sanitize(test_name)));
    image::save_buffer(&path, rgba, width, height, image::ColorType::Rgba8)
        .map_err(|err| err.to_string())
}

/// Map `test_name` to a safe file name: keep `[A-Za-z0-9._-]`, replace
/// everything else (`::`, `/`, `\`, whitespace, …) with `_`.
fn sanitize(test_name: &str) -> String {
    test_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dump_dir() -> PathBuf {
        std::env::temp_dir().join(format!("flui-readback-dump-test-{}", std::process::id()))
    }

    #[test]
    fn dumps_png_to_the_given_dir_and_stays_quiet_without_one() {
        let dir = unique_dump_dir();
        // Clean slate in case a previous run was interrupted.
        let _ = std::fs::remove_dir_all(&dir);

        // 2×2 RGBA buffer with distinct pixels.
        let pixels: [u8; 16] = [
            255, 0, 0, 255, // opaque red
            0, 255, 0, 128, // half-alpha green
            0, 0, 255, 64, // quarter-alpha blue
            255, 255, 0, 0, // transparent yellow
        ];

        // Without a directory nothing is written — not even the directory.
        dump_rgba_png_to(None, "wgpu::tests::no_env_case", 2, 2, &pixels);
        assert!(!dir.exists(), "no dump may be written without a directory");

        // With one, the frame lands as a decodable PNG under the sanitized
        // test name.
        dump_rgba_png_to(Some(&dir), "wgpu::tests::sample_frame", 2, 2, &pixels);
        let png_path = dir.join("wgpu__tests__sample_frame.png");
        let bytes = std::fs::read(&png_path).expect("dump PNG must exist");
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "dump must start with the PNG magic bytes"
        );
        let decoded = image::open(&png_path).expect("dump must decode as a PNG");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        assert_eq!(decoded.to_rgba8().into_raw(), pixels);

        std::fs::remove_dir_all(&dir).expect("temp dump dir must be removable");
    }
}
