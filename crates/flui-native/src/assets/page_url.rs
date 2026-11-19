//! The page-relative URL of a Web package's asset (ADR-0384 §3), apart from the
//! fetch so the Linux host runs its tests.

use flui_platform::assets::AssetKey;

use super::PACKAGE_DIR;

/// The URL of `key` beside the page whose directory is `base` (ending in
/// `/`): `<base>flui_assets/<key>`, each segment percent-encoded.
pub(super) fn asset_url(base: &str, key: &AssetKey) -> String {
    let mut url = format!("{base}{PACKAGE_DIR}");
    for segment in key.segments() {
        url.push('/');
        percent_encode(segment, &mut url);
    }
    url
}

/// The directory of `page_url`: everything up to its last `/` before any
/// query or fragment.
pub(super) fn page_directory(page_url: &str) -> &str {
    let end = page_url.find(['?', '#']).unwrap_or(page_url.len());
    let path = &page_url[..end];
    path.rfind('/').map_or(path, |slash| &path[..=slash])
}

/// Appends `segment` to `url`, every byte outside RFC 3986's unreserved set
/// percent-encoded.
fn percent_encode(segment: &str, url: &mut String) {
    use std::fmt::Write;
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::assets::AssetKey;

    use super::{asset_url, page_directory};

    /// The page's directory drops the page's own name, its query and its
    /// fragment, and a key's segments are encoded one by one, so no key
    /// reaches a URL outside `flui_assets/` or another origin.
    ///
    /// Red-check: keep the query in the base, or leave `%` or `#`
    /// unencoded in a segment.
    #[test]
    fn an_asset_url_stays_under_the_pages_flui_assets() {
        assert_eq!(
            page_directory("https://example.com/app/index.html"),
            "https://example.com/app/"
        );
        assert_eq!(
            page_directory("https://example.com/app/?page=a/b#x/y"),
            "https://example.com/app/"
        );
        assert_eq!(
            page_directory("https://example.com/"),
            "https://example.com/"
        );
        let key = AssetKey::try_new("assets/фото 1#%.png").expect("a valid key");
        assert_eq!(
            asset_url("https://example.com/app/", &key),
            "https://example.com/app/flui_assets/assets/%D1%84%D0%BE%D1%82%D0%BE%201%23%25.png"
        );
    }
}
