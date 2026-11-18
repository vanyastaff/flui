//! Where a host bounds what the operating system opened the application with
//! and reports what bounding dropped.
//!
//! The contract bounds ([`Opened::bounded`]) and hands the count back; the host
//! logs it here, at the site that bounds, so nothing the operating system
//! offered is dropped unseen: a launch whose every item was dropped reports
//! it too, and a second instance reports what it dropped before it forwards
//! the rest.

use flui_platform::{
    Launch, MAX_OPENED_ITEMS, MAX_OPENED_URL_LEN, OpenTarget, Opened, OpenedFile, OpenedOverflow,
};

/// The target of the log line [`report`] writes, which a test counts.
pub(crate) const LOG_TARGET: &str = "flui_native::opened";

/// What the operating system opened, bounded ([`Opened::bounded`]), with
/// what bounding dropped reported. `unread` counts the items past the bound
/// the host never turned into files (an Android document it does not open
/// once no room is left); they are reported as past the limit.
pub(crate) fn bounded(
    urls: impl IntoIterator<Item = String>,
    files: impl IntoIterator<Item = OpenedFile>,
    unread: usize,
    target: OpenTarget,
    launch: Launch,
) -> Opened {
    let (opened, mut overflow) = Opened::bounded(urls, files, target, launch);
    overflow.past_limit = overflow.past_limit.saturating_add(unread);
    report(overflow);
    opened
}

/// The room an [`Opened`] leaves for files after `urls`: the items it takes
/// less the URLs it keeps (an over-long URL takes no room).
#[cfg(any(target_os = "android", all(test, not(target_arch = "wasm32"))))]
pub(crate) fn room_for_files<'a>(urls: impl IntoIterator<Item = &'a String>) -> usize {
    let kept = urls
        .into_iter()
        .filter(|url| url.len() <= MAX_OPENED_URL_LEN)
        .count();
    MAX_OPENED_ITEMS.saturating_sub(kept)
}

/// Log `overflow` unless nothing was dropped.
fn report(overflow: OpenedOverflow) {
    if !overflow.is_empty() {
        tracing::warn!(
            target: LOG_TARGET,
            too_long = overflow.too_long,
            past_limit = overflow.past_limit,
            max_items = MAX_OPENED_ITEMS,
            max_url_len = MAX_OPENED_URL_LEN,
            "the operating system opened more than the application takes; the rest is dropped"
        );
    }
}

/// Counts the lines [`report`] writes on this thread while `body` runs.
#[cfg(test)]
pub(crate) fn reports_in(body: impl FnOnce()) -> usize {
    crate::testing::log::lines_in(LOG_TARGET, body)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use flui_platform::MAX_OPENED_ITEMS;

    use super::reports_in;
    use crate::owner::file_uri::PathConvention;
    use crate::owner::launch_arguments::opened_by_arguments;

    /// Launch arguments past the bound are dropped and reported once, by the
    /// launch that bounds them; a launch within it reports nothing.
    #[test]
    fn a_launch_reports_what_bounding_its_arguments_dropped() {
        let launch = |count: usize| {
            let mut opened = None;
            let reports = reports_in(|| {
                opened = Some(opened_by_arguments(
                    (0..count).map(|n| OsString::from(format!("notes:///note/{n}"))),
                    PathConvention::Unix,
                    |_| false,
                    |path| Some(path.to_owned()),
                ));
            });
            (opened.expect("bounded").urls.len(), reports)
        };
        assert_eq!(launch(MAX_OPENED_ITEMS + 36), (MAX_OPENED_ITEMS, 1));
        assert_eq!(launch(MAX_OPENED_ITEMS), (MAX_OPENED_ITEMS, 0));
    }
}
