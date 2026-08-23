//! Pinning the shared font system so a committed layout is reproducible.
//!
//! # Why a test has to care
//!
//! `flui-painting`'s process-wide `FontSystem` is built from whatever fonts
//! the *host machine* has installed. Text measurement runs against it, and
//! anything sized to its text — a button hugging its label, a centered row, an
//! app bar's title — takes its geometry from the resulting advance widths. So
//! the same tree, built from the same source on two machines with different
//! font sets, lays out differently.
//!
//! That is not hypothetical. Measured on this repository's demo trees, the
//! same Cupertino button was 61.18 px wide on a host with fonts installed and
//! 129.55 px on a host without; the string a demo renders measured 24 px
//! (9.4%) wider against the fonts of the machine that captured this
//! repository's old golden PNGs than against a Linux container's. Any test
//! that commits *layout* — a layer-tree snapshot, a geometry assertion tighter
//! than a few pixels, a pixel golden — is pinned to its author's font
//! installation until the face set is pinned instead.
//!
//! [`pin_font_faces`] is that pin.

/// Builds the process-wide font system from `faces` alone, so text
/// measurement resolves against repository-shipped bytes on every host.
///
/// Call this **before any text is measured or shaped in the process**, and
/// before anything else touches the font system: it initializes the shared
/// `FontSystem` rather than editing one, because a `FontSystem` freezes its
/// fallback chain and monospace face list at construction and no later
/// database edit reaches them (see
/// [`init_font_system_with_faces`](flui_painting::text_layout::init_font_system_with_faces)
/// for the measurements behind that). In practice: call it at the top of a
/// test, guarded by a `std::sync::Once` when several tests share the binary.
///
/// `faces` are raw font-file bytes; `flui_engine::fonts` exposes the faces
/// this repository ships. `default_family` must name a family one of `faces`
/// provides — it becomes the target of every generic family, so text whose
/// style names no family cannot fall through to a host font.
///
/// # Panics
///
/// Panics if `faces` is empty, if none of them load, or if the font system was
/// already initialized. The last is deliberate: a pin that silently did
/// nothing would leave the test measuring against host fonts while reading as
/// though it had been pinned, which is the exact failure this exists to
/// prevent.
pub fn pin_font_faces(faces: &[&[u8]], default_family: &str) {
    assert!(
        flui_painting::text_layout::init_font_system_with_faces(faces, default_family, "en-US"),
        "pin_font_faces: the shared font system was already initialized, so this \
         pin changed nothing and measurement would still resolve against the \
         host's fonts. Pin before the first text is measured — earlier in the \
         test, or before the code that shaped text first.",
    );
}
