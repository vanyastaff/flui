//! Where a stretch of work allocates, by source line.
//!
//! With this crate's `dhat` feature, [`CountingAllocator`](super::CountingAllocator) forwards
//! to `dhat::Alloc`, which records the stack of every allocation while a [`SiteProfiler`]
//! runs. [`SiteProfiler::finish`] writes the DHAT profile (open it in DHAT's viewer,
//! `dh_view.html`, for whole stacks and lifetimes), reads it back, resolves its addresses
//! again in this process for whole symbols and paths, and sums the allocations per site: the
//! first frame of a stack that is neither the allocator, the standard library nor a
//! dependency, which is the line of FLUI code that asked.
//!
//! Without the feature [`SiteProfiler::start`] answers `None`, so a bench or a test calls it
//! unconditionally and a run turns the feature on from the command line:
//!
//! ```text
//! cargo bench -p flui-material --bench app_scenarios --features flui-testing/dhat
//! ```
//!
//! Recording a stack per allocation makes the profiled work many times slower; its timings
//! mean nothing, its counts are exact.

use std::fmt;
use std::path::{Path, PathBuf};

/// How many sites [`AllocationSites`]'s `Display` lists.
const SITES_SHOWN: usize = 20;

/// The allocations one site of FLUI code asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationSite {
    /// The function the site is in, as the symbol names it.
    pub function: String,
    /// `path:line` of the call that allocated, the path from the workspace root.
    pub location: String,
    /// Allocations, a growth by `realloc` included.
    pub blocks: u64,
    /// The bytes those allocations asked for.
    pub bytes: u64,
}

/// A profile's allocations summed per site, most allocations first.
#[derive(Clone, Debug)]
pub struct AllocationSites {
    sites: Vec<AllocationSite>,
    profile: PathBuf,
}

impl AllocationSites {
    /// Reads a DHAT heap profile (`dhatFileVersion` 2, as [`SiteProfiler`] or
    /// `valgrind --tool=dhat` writes it) and sums per site the allocations whose stack
    /// passes through a function whose name contains `within` (all of them when it is empty).
    ///
    /// The frames are taken as the profile writes them. Valgrind's DHAT writes whole paths;
    /// dhat-rs writes short symbols and a path's last three components, so a profile it wrote
    /// in another process can name a standard-library frame (`src/vec/mod.rs`) as the site.
    /// [`SiteProfiler::finish`] resolves the addresses again instead.
    ///
    /// # Errors
    ///
    /// When the profile cannot be read or is not DHAT's JSON.
    pub fn read(profile: impl Into<PathBuf>, within: &str) -> Result<Self, SiteProfileError> {
        Self::summed(profile.into(), within, stacks_as_written)
    }

    fn summed(
        path: PathBuf,
        within: &str,
        stacks: impl FnOnce(&DhatProfile) -> Vec<Vec<Frame>>,
    ) -> Result<Self, SiteProfileError> {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) => return Err(SiteProfileError::Read { path, source }),
        };
        match serde_json::from_str::<DhatProfile>(&text) {
            Ok(parsed) => Ok(Self {
                sites: sum_sites(&parsed, &stacks(&parsed), within),
                profile: path,
            }),
            Err(source) => Err(SiteProfileError::Parse { path, source }),
        }
    }

    /// Every site, most allocations first; ties by bytes, then by location.
    #[must_use]
    pub fn sites(&self) -> &[AllocationSite] {
        &self.sites
    }

    /// The DHAT profile the sites were read from.
    #[must_use]
    pub fn profile(&self) -> &Path {
        &self.profile
    }
}

impl fmt::Display for AllocationSites {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let blocks: u64 = self.sites.iter().map(|site| site.blocks).sum();
        let bytes: u64 = self.sites.iter().map(|site| site.bytes).sum();
        writeln!(
            f,
            "{blocks} allocations, {bytes} bytes, from {} sites (profile: {})",
            self.sites.len(),
            self.profile.display()
        )?;
        for site in self.sites.iter().take(SITES_SHOWN) {
            writeln!(
                f,
                "{:>8} {:>10} B  {}  {}",
                site.blocks, site.bytes, site.location, site.function
            )?;
        }
        Ok(())
    }
}

/// Why a profile could not be read back.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SiteProfileError {
    /// The profile file could not be read.
    #[error("reading the DHAT profile {path}: {source}")]
    Read {
        /// The profile's path.
        path: PathBuf,
        /// The I/O failure.
        source: std::io::Error,
    },
    /// The profile is not DHAT's JSON.
    #[error("parsing the DHAT profile {path}: {source}")]
    Parse {
        /// The profile's path.
        path: PathBuf,
        /// The parse failure.
        source: serde_json::Error,
    },
}

/// A DHAT heap profile in progress. At most one runs in a process at a time.
#[must_use = "the profile is written when `finish` is called"]
pub struct SiteProfiler {
    #[cfg(feature = "dhat")]
    profiler: dhat::Profiler,
    #[cfg(not(feature = "dhat"))]
    never: std::convert::Infallible,
    path: PathBuf,
}

impl fmt::Debug for SiteProfiler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SiteProfiler")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl SiteProfiler {
    /// Starts recording the stack of every allocation, to be written to `path`; `None`
    /// without this crate's `dhat` feature.
    ///
    /// # Panics
    ///
    /// When another profiler is still running in this process (DHAT keeps one).
    pub fn start(path: impl Into<PathBuf>) -> Option<Self> {
        let path = path.into();
        #[cfg(feature = "dhat")]
        {
            if let Some(dir) = path.parent() {
                // A directory that cannot be made fails the profile's own write, which
                // `finish` reports; the run goes on until then.
                let _ = std::fs::create_dir_all(dir);
            }
            let profiler = dhat::Profiler::builder()
                .file_name(&path)
                // Whole stacks: a site is found below the allocator's frames, and `within`
                // is looked for anywhere in the stack.
                .trim_backtraces(None)
                .build();
            Some(Self { profiler, path })
        }
        #[cfg(not(feature = "dhat"))]
        {
            drop(path);
            None
        }
    }

    /// Stops recording, writes the profile, and sums per site the allocations whose stack
    /// passes through a function whose name contains `within` (all of them when it is empty).
    ///
    /// # Errors
    ///
    /// When the written profile cannot be read back or parsed.
    pub fn finish(self, within: &str) -> Result<AllocationSites, SiteProfileError> {
        #[cfg(feature = "dhat")]
        {
            let Self { profiler, path } = self;
            // DHAT writes the profile when its profiler drops.
            drop(profiler);
            AllocationSites::summed(path, within, stacks_resolved_here)
        }
        #[cfg(not(feature = "dhat"))]
        {
            let _ = within;
            match self.never {}
        }
    }
}

/// The part of DHAT's JSON (`dhatFileVersion` 2) a summary reads.
#[derive(serde::Deserialize)]
struct DhatProfile {
    /// One program point: a distinct stack and what was allocated through it.
    pps: Vec<ProgramPoint>,
    /// Every frame, as `0xADDR: symbol (file:line:col)`; `pps[].fs` index it.
    ftbl: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ProgramPoint {
    /// Total bytes.
    tb: u64,
    /// Total blocks.
    tbk: u64,
    /// Frames, innermost first.
    fs: Vec<usize>,
}

/// One frame of a stack, innermost first in a [`Vec`].
#[derive(Clone, Debug)]
struct Frame {
    function: String,
    file: String,
    line: u32,
}

/// The address and frame of one `ftbl` entry.
fn parse_frame(text: &str) -> Option<(u64, Frame)> {
    // dhat-rs: `0x55d0c3a2b1f0: move_to (src/paint/path.rs:718:9)`; Valgrind's DHAT:
    // `0x4C2DB8F: malloc (vg_replace_malloc.c:299)`, no column.
    let (address, rest) = text.split_once(": ")?;
    let address = u64::from_str_radix(address.strip_prefix("0x")?, 16).ok()?;
    let (function, location) = rest.rsplit_once(" (")?;
    let location = location.strip_suffix(')')?;
    let (head, last) = location.rsplit_once(':')?;
    let (file, line) = match head.rsplit_once(':') {
        Some((file, line)) if line.bytes().all(|byte| byte.is_ascii_digit()) => (file, line),
        _ => (head, last),
    };
    Some((
        address,
        Frame {
            function: function.to_owned(),
            file: file.to_owned(),
            line: line.parse::<u32>().ok()?,
        },
    ))
}

/// The stacks of a profile's program points as its text gives them.
fn stacks_as_written(profile: &DhatProfile) -> Vec<Vec<Frame>> {
    let table: Vec<Option<Frame>> = profile
        .ftbl
        .iter()
        .map(|text| parse_frame(text).map(|(_, frame)| frame))
        .collect();
    profile
        .pps
        .iter()
        .map(|point| {
            point
                .fs
                .iter()
                .filter_map(|&index| table.get(index).cloned().flatten())
                .collect::<Vec<Frame>>()
        })
        .collect()
}

/// The stacks of a profile this process recorded, each address resolved again in full.
///
/// dhat-rs writes a symbol without its path and keeps only a file's last three components
/// (`src/vec/mod.rs`, `src/paint/path.rs`), which cannot tell the standard library, a
/// dependency and FLUI apart. The addresses are this process's own, so `backtrace` gives the
/// whole symbol and path; an address it cannot resolve keeps its written frames. An address
/// is a run of entries in a stack, one per frame inlined at it.
#[cfg(feature = "dhat")]
fn stacks_resolved_here(profile: &DhatProfile) -> Vec<Vec<Frame>> {
    use rustc_hash::FxHashMap;

    let table: Vec<Option<(u64, Frame)>> =
        profile.ftbl.iter().map(|text| parse_frame(text)).collect();
    let mut resolved: FxHashMap<u64, Vec<Frame>> = FxHashMap::default();
    let mut resolve = |address: u64| -> Vec<Frame> {
        resolved
            .entry(address)
            .or_insert_with(|| {
                let mut frames = Vec::new();
                // The address is one this process recorded; `resolve` only reads its
                // debug info.
                backtrace::resolve(address as usize as *mut std::ffi::c_void, |symbol| {
                    let (Some(name), Some(file), Some(line)) =
                        (symbol.name(), symbol.filename(), symbol.lineno())
                    else {
                        return;
                    };
                    frames.push(Frame {
                        function: format!("{name:#}"),
                        file: file.display().to_string(),
                        line,
                    });
                });
                frames
            })
            .clone()
    };
    profile
        .pps
        .iter()
        .map(|point| {
            let mut stack = Vec::new();
            let entries: Vec<&(u64, Frame)> = point
                .fs
                .iter()
                .filter_map(|&index| table.get(index).and_then(Option::as_ref))
                .collect();
            for run in entries.chunk_by(|a, b| a.0 == b.0) {
                let frames = resolve(run[0].0);
                if frames.is_empty() {
                    stack.extend(run.iter().map(|(_, frame)| frame.clone()));
                } else {
                    stack.extend(frames);
                }
            }
            stack
        })
        .collect()
}

/// Whether a frame belongs to the allocation machinery, the standard library or a
/// dependency, rather than to the code that asked for the memory.
fn is_below_the_asker(frame: &Frame) -> bool {
    const MACHINERY: [&str; 9] = [
        "dhat::",
        "alloc::",
        "core::",
        "std::",
        "<alloc::",
        "<core::",
        "<std::",
        "__rust",
        "__rdl",
    ];
    let file = frame.file.replace('\\', "/");
    MACHINERY
        .iter()
        .any(|prefix| frame.function.starts_with(prefix))
        // The standard library, from its own build or from `rust-src`.
        || file.starts_with("/rustc/")
        || file.contains("/library/alloc/")
        || file.contains("/library/core/")
        || file.contains("/library/std/")
        || file.contains("/.cargo/registry/")
        || file.contains("/.cargo/git/")
        // The counting allocator itself.
        || file.ends_with("flui-testing/src/heap.rs")
        || file == "???"
}

/// `crates/…` or `packages/…` of a workspace path, else the path as DHAT wrote it.
fn workspace_relative(file: &str) -> &str {
    ["crates/", "packages/", "src/", "examples/", "tests/"]
        .iter()
        .find_map(|root| file.find(root).map(|at| &file[at..]))
        .unwrap_or(file)
}

fn sum_sites(profile: &DhatProfile, stacks: &[Vec<Frame>], within: &str) -> Vec<AllocationSite> {
    use std::collections::BTreeMap;

    let mut sums: BTreeMap<(String, String), (u64, u64)> = BTreeMap::new();
    for (point, stack) in profile.pps.iter().zip(stacks) {
        if !within.is_empty() && !stack.iter().any(|frame| frame.function.contains(within)) {
            continue;
        }
        let key = stack
            .iter()
            .find(|frame| !is_below_the_asker(frame))
            .map_or_else(
                || ("(no FLUI frame)".to_owned(), String::new()),
                |frame| {
                    (
                        format!("{}:{}", workspace_relative(&frame.file), frame.line),
                        frame.function.clone(),
                    )
                },
            );
        let sum = sums.entry(key).or_default();
        sum.0 = sum.0.saturating_add(point.tbk);
        sum.1 = sum.1.saturating_add(point.tb);
    }
    let mut sites: Vec<AllocationSite> = sums
        .into_iter()
        .map(|((location, function), (blocks, bytes))| AllocationSite {
            function,
            location,
            blocks,
            bytes,
        })
        .collect();
    sites.sort_by(|a, b| {
        b.blocks
            .cmp(&a.blocks)
            .then(b.bytes.cmp(&a.bytes))
            .then_with(|| a.location.cmp(&b.location))
    });
    sites
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(json: &str) -> DhatProfile {
        serde_json::from_str::<DhatProfile>(json).expect("a DHAT profile")
    }

    /// Allocations are summed at the first FLUI frame under the allocator, the standard
    /// library and dependencies, and only stacks through `within` count.
    #[test]
    fn allocations_are_summed_at_the_first_flui_frame_within_the_marker() {
        let profile = profile(
            r#"{
              "pps": [
                {"tb": 64, "tbk": 2, "fs": [1, 2, 3, 4, 5]},
                {"tb": 32, "tbk": 1, "fs": [1, 2, 3, 4, 5]},
                {"tb": 16, "tbk": 1, "fs": [1, 6, 5]},
                {"tb": 999, "tbk": 9, "fs": [1, 4, 7]}
              ],
              "ftbl": [
                "[root]",
                "0x1: <flui_testing::heap::CountingAllocator as core::alloc::global::GlobalAlloc>::alloc (crates/flui-testing/src/heap.rs:90:9)",
                "0x2: alloc::raw_vec::RawVec<T>::grow (/rustc/abc/library/alloc/src/raw_vec.rs:10:5)",
                "0x3: <alloc::vec::Vec<T> as kurbo::Shape>::push (/root/.cargo/registry/src/x/kurbo-0.13.1/src/bezpath.rs:1:1)",
                "0x4: flui_view::tree::build (crates/flui-view/src/tree.rs:120:9)",
                "0x5: app_scenarios::scenario_measured (/home/user/flui/packages/flui-material/benches/app_scenarios.rs:60:5)",
                "0x6: smallvec::SmallVec<A>::push (/root/.cargo/registry/src/x/smallvec-1.16.3/src/lib.rs:1:1)",
                "0x7: app_scenarios::mount (/home/user/flui/packages/flui-material/benches/app_scenarios.rs:99:5)"
              ]
            }"#,
        );
        let stacks = stacks_as_written(&profile);
        let sites = sum_sites(&profile, &stacks, "scenario_measured");
        assert_eq!(
            sites,
            vec![
                AllocationSite {
                    function: "flui_view::tree::build".to_owned(),
                    location: "crates/flui-view/src/tree.rs:120".to_owned(),
                    blocks: 3,
                    bytes: 96,
                },
                AllocationSite {
                    function: "app_scenarios::scenario_measured".to_owned(),
                    location: "packages/flui-material/benches/app_scenarios.rs:60".to_owned(),
                    blocks: 1,
                    bytes: 16,
                },
            ]
        );
        let (address, valgrind) = parse_frame("0x4C2DB8F: malloc (vg_replace_malloc.c:299)")
            .expect("Valgrind's frame form, no column");
        assert_eq!(
            (address, valgrind.function.as_str(), valgrind.file.as_str(), valgrind.line),
            (0x04C2_DB8F, "malloc", "vg_replace_malloc.c", 299)
        );
        let everything = sum_sites(&profile, &stacks, "");
        assert_eq!(
            everything.iter().map(|site| site.blocks).sum::<u64>(),
            13,
            "an empty `within` keeps every stack"
        );
    }
}
