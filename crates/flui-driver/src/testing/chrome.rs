//! The Chrome lane's internals its tests reach (ADR-0243): the ChromeDriver resolution and fetch,
//! with the fetch pointed at stand-in hosts on the loopback, and the page's server. None of it is
//! the driver's API: a launch reaches them through `Launch::chromedriver` and
//! `Artifact::WasmBundle`.

use std::path::{Path, PathBuf};

pub use crate::lane::chrome::chromedriver::{
    Chrome, MAX_ARCHIVE, archive_url, chrome_args, executable_name, extract, is_version,
    newest_version, reported_port, verify_kept,
};
pub use crate::lane::chrome::page::{
    CONNECTION_DEADLINE, MAX_HEAD, PageServer, READ_TIMEOUT, resolve as page_file,
};

use crate::error::Result;
use crate::lane::chrome::chromedriver::{self, Fetcher};

/// The ChromeDriver for `chrome`, as a launch resolves it: `named` when given, else the one kept
/// in `cache` once its hash matches its record, else, when `archives` names stand-in hosts on the
/// loopback (`http://127.0.0.1:<port>`, serving `/archives/…` and `/latest/LATEST_RELEASE_…`),
/// the one fetched from them for `platform`'s archive name.
///
/// # Errors
///
/// As the launch's resolution; [`crate::Error::InvalidArgument`] for hosts off the loopback.
pub fn resolve_chromedriver(
    chrome: &Chrome,
    cache: &Path,
    named: Option<PathBuf>,
    archives: Option<&str>,
) -> Result<PathBuf> {
    let fetcher = archives.map(Fetcher::loopback).transpose()?;
    chromedriver::resolve(chrome, cache, named, fetcher.as_ref())
}

/// Put `bytes` at `dir/name` as a fetch puts a cache file, on a filesystem that refuses hard links
/// (FAT32, exFAT, some SMB and FUSE mounts): created in place, never over another's.
///
/// # Errors
///
/// As a fetch's write of the cache: [`crate::Error::Lane`] marked as a host that cannot run the
/// lane when the cache cannot be written or another fetch's file differs.
pub fn place_without_links(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    chromedriver::place(dir, name, bytes, false, |_, _| {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    })
}

/// The Chrome for Testing name of this host's platform, which a stand-in serves its archive under.
#[must_use]
pub fn platform() -> Option<&'static str> {
    chromedriver::platform()
}
