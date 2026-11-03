//! The page alignment of an Android native library.
//!
//! A device with 16 KB memory pages maps each `PT_LOAD` segment of a library
//! at a page boundary, so a segment aligned to 4 KB fails to load there, and
//! Google Play refuses an app whose 64-bit libraries are not 16 KB-aligned.
//! The alignment is the linker's (NDK r28 and later link 16 KB by default),
//! not something `flui build` sets, so the build reads it back from each
//! library and refuses one that would not load, instead of shipping it.

use std::path::Path;

use crate::build::error::{BuildError, BuildResult};

/// The page size 64-bit Android libraries must be aligned for.
const PAGE_SIZE: u64 = 16 * 1024;

/// `p_type` of a loadable segment.
const PT_LOAD: u32 = 1;
/// Size of the ELF64 header.
const EHDR_SIZE: usize = 64;
/// Size of an ELF64 program header the format defines.
const PHDR_SIZE: usize = 56;

/// Whether libraries of `abi` must be 16 KB-aligned: the 64-bit ABIs, the
/// only ones a 16 KB-page kernel runs.
pub(crate) fn needs_16_kb_pages(abi: &str) -> bool {
    matches!(
        abi,
        "arm64-v8a" | "x86_64" | "aarch64-linux-android" | "x86_64-linux-android"
    )
}

/// Refuses `library` when one of its loadable segments is aligned below
/// [`PAGE_SIZE`].
pub(crate) fn check_page_alignment(library: &Path) -> BuildResult<()> {
    let bytes = std::fs::read(library)?;
    let alignment = min_load_alignment(&bytes)
        .map_err(|reason| BuildError::Other(format!("{}: {reason}", library.display())))?;
    if alignment < PAGE_SIZE {
        return Err(BuildError::PageAlignment {
            library: library.to_path_buf(),
            alignment,
        });
    }
    Ok(())
}

/// The smallest `p_align` among the `PT_LOAD` segments of a little-endian
/// ELF64 image.
fn min_load_alignment(elf: &[u8]) -> Result<u64, String> {
    let header = elf.get(..EHDR_SIZE).ok_or("shorter than an ELF64 header")?;
    if header[..4] != *b"\x7fELF" {
        return Err("not an ELF file".to_owned());
    }
    // EI_CLASS 2 is ELF64, EI_DATA 1 little-endian: every 64-bit Android ABI.
    if header[4] != 2 || header[5] != 1 {
        return Err("not a little-endian ELF64 file".to_owned());
    }
    let phoff = usize::try_from(read_u64(header, 0x20)).map_err(|_| "e_phoff out of range")?;
    let phentsize = usize::from(read_u16(header, 0x36));
    let phnum = usize::from(read_u16(header, 0x38));
    if phentsize < PHDR_SIZE {
        return Err(format!("e_phentsize {phentsize} is below {PHDR_SIZE}"));
    }
    let table_len = phentsize
        .checked_mul(phnum)
        .ok_or("program header table size overflows")?;
    let table = phoff
        .checked_add(table_len)
        .and_then(|end| elf.get(phoff..end))
        .ok_or("program header table runs past the end of the file")?;
    table
        .chunks_exact(phentsize)
        .filter(|entry| read_u32(entry, 0) == PT_LOAD)
        .map(|entry| read_u64(entry, 0x30))
        .min()
        .ok_or_else(|| "no loadable segment".to_owned())
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(
        bytes[at..at + 2]
            .try_into()
            .expect("BUG: the caller checked the length"),
    )
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("BUG: the caller checked the length"),
    )
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(
        bytes[at..at + 8]
            .try_into()
            .expect("BUG: the caller checked the length"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little-endian ELF64 image whose program headers are `segments`, each
    /// `(p_type, p_align)`.
    fn elf(segments: &[(u32, u64)]) -> Vec<u8> {
        let mut image = vec![0_u8; EHDR_SIZE];
        image[..4].copy_from_slice(b"\x7fELF");
        image[4] = 2;
        image[5] = 1;
        image[0x20..0x28].copy_from_slice(&(EHDR_SIZE as u64).to_le_bytes());
        image[0x36..0x38].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
        image[0x38..0x3a].copy_from_slice(
            &u16::try_from(segments.len())
                .expect("few segments")
                .to_le_bytes(),
        );
        for (kind, align) in segments {
            let mut entry = vec![0_u8; PHDR_SIZE];
            entry[..4].copy_from_slice(&kind.to_le_bytes());
            entry[0x30..0x38].copy_from_slice(&align.to_le_bytes());
            image.extend(entry);
        }
        image
    }

    fn the_smallest_loadable_alignment_is_read() {
        // PT_PHDR (6) and PT_GNU_STACK carry alignments that are not pages.
        let image = elf(&[
            (6, 8),
            (PT_LOAD, 16384),
            (PT_LOAD, 16384),
            (0x6474_e551, 16),
        ]);
        assert_eq!(min_load_alignment(&image), Ok(16384));
        let image = elf(&[(PT_LOAD, 16384), (PT_LOAD, 4096)]);
        assert_eq!(min_load_alignment(&image), Ok(4096));
    }

    fn a_library_aligned_for_4_kb_pages_is_refused() {
        let dir = tempfile::tempdir().expect("temp");
        let library = dir.path().join("libapp.so");
        std::fs::write(&library, elf(&[(PT_LOAD, 4096), (PT_LOAD, 4096)])).expect("write");
        match check_page_alignment(&library) {
            Err(BuildError::PageAlignment {
                library: refused,
                alignment: 4096,
            }) => assert_eq!(refused, library),
            other => panic!("a 4 KB-aligned library was not refused: {other:?}"),
        }
        std::fs::write(&library, elf(&[(PT_LOAD, 16384), (PT_LOAD, 65536)])).expect("write");
        check_page_alignment(&library).expect("a 16 KB-aligned library is accepted");
    }

    fn a_file_that_is_not_an_elf64_library_is_refused() {
        let mut elf32 = elf(&[(PT_LOAD, 16384)]);
        elf32[4] = 1;
        let mut truncated = elf(&[(PT_LOAD, 16384)]);
        truncated.truncate(EHDR_SIZE + 10);
        let mut huge = elf(&[(PT_LOAD, 16384)]);
        huge[0x20..0x28].copy_from_slice(&u64::MAX.to_le_bytes());
        for (image, needle) in [
            (b"\x7fELF".to_vec(), "shorter than"),
            (vec![0_u8; 128], "not an ELF file"),
            (elf32, "not a little-endian ELF64"),
            (truncated, "runs past the end"),
            (huge, "runs past the end"),
            (elf(&[(6, 8)]), "no loadable segment"),
        ] {
            let error = min_load_alignment(&image).expect_err(needle);
            assert!(error.contains(needle), "{needle}: {error}");
        }
    }

    fn only_64_bit_abis_need_16_kb_pages() {
        assert!(needs_16_kb_pages("arm64-v8a"));
        assert!(needs_16_kb_pages("x86_64"));
        assert!(!needs_16_kb_pages("armeabi-v7a"));
        assert!(!needs_16_kb_pages("x86"));
    }

    #[test]
    fn android_libraries_load_on_16_kb_pages() {
        crate::test_cases::run_cases(&[
            (
                "the smallest loadable alignment is read",
                the_smallest_loadable_alignment_is_read,
            ),
            (
                "a library aligned for 4 KB pages is refused",
                a_library_aligned_for_4_kb_pages_is_refused,
            ),
            (
                "a file that is not an ELF64 library is refused",
                a_file_that_is_not_an_elf64_library_is_refused,
            ),
            (
                "only 64-bit ABIs need 16 KB pages",
                only_64_bit_abis_need_16_kb_pages,
            ),
        ]);
    }
}
