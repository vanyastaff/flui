//! The page a web build serves, written whole on every build (ADR-0255
//! decision 16): a mount point and a loader. The page holds no canvas; the
//! host makes its own root in the body when the module starts. It asks to
//! cover the whole display (`viewport-fit=cover`), so the host reads the
//! safe area from `env(safe-area-inset-*)`; its `<base href>` resolves the
//! module and the manifest from the deployment's root path whatever address
//! the page is served at, so an application can be hosted under a sub-path
//! and answer a deep link with the same page.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The path a web build is served from: absolute, starting and ending with
/// `/` (`/`, `/apps/demo/`), with no character the page would have to
/// escape or a URL would read as more than a path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct BaseHref(String);

/// Why a base path was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InvalidBaseHref {
    /// It does not start and end with `/`.
    #[error("`{0}` must start and end with `/` (`/`, `/apps/demo/`)")]
    NotADirectory(String),
    /// It holds a character a path must not (a quote, an angle bracket, a
    /// space, a query, a fragment, a backslash or a control character).
    #[error("`{0}` holds a character a base path cannot (quotes, `<>`, spaces, `?`, `#`, `\\`)")]
    Character(String),
}

impl BaseHref {
    /// The site's root, where `flui run` serves a build.
    pub(crate) fn root() -> Self {
        Self("/".to_owned())
    }

    /// `path` as a base path.
    ///
    /// # Errors
    /// [`InvalidBaseHref`] when `path` does not start and end with `/`, or
    /// holds a character a base path cannot.
    pub(crate) fn try_new(path: impl Into<String>) -> Result<Self, InvalidBaseHref> {
        let path = path.into();
        if !path.starts_with('/') || !path.ends_with('/') {
            return Err(InvalidBaseHref::NotADirectory(path));
        }
        let refused = |c: char| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, '"' | '\'' | '<' | '>' | '&' | '?' | '#' | '\\' | '`')
        };
        if path.chars().any(refused) {
            return Err(InvalidBaseHref::Character(path));
        }
        Ok(Self(path))
    }
}

impl Default for BaseHref {
    fn default() -> Self {
        Self::root()
    }
}

impl TryFrom<String> for BaseHref {
    type Error = InvalidBaseHref;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        Self::try_new(path)
    }
}

impl From<BaseHref> for String {
    fn from(base: BaseHref) -> Self {
        base.0
    }
}

impl fmt::Display for BaseHref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The directory, relative to the page's base, that `wasm-bindgen` writes
/// the module into.
pub(crate) const PKG_DIR: &str = "pkg";
/// The module's name: `wasm-bindgen --out-name` writes `<PKG_DIR>/<OUT_NAME>.js`,
/// which the page imports.
pub(crate) const OUT_NAME: &str = "app";
/// The web app manifest the page links, relative to its base.
pub(crate) const MANIFEST: &str = "manifest.json";

/// The page of an application named `title`, served from `base`.
#[must_use]
pub(crate) fn render(title: &str, base: &BaseHref) -> String {
    let title = escape(title);
    format!(
        r#"<!DOCTYPE html>
<!-- Written by `flui build` on every build; edits here are lost. -->
<html lang="en">
<head>
    <meta charset="UTF-8">
    <base href="{base}">
    <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
    <meta name="mobile-web-app-capable" content="yes">
    <meta name="apple-mobile-web-app-title" content="{title}">
    <title>{title}</title>
    <link rel="manifest" href="{MANIFEST}">
    <style>
        html, body {{ margin: 0; height: 100%; overflow: hidden; overscroll-behavior: none; }}
    </style>
</head>
<body>
    <script type="module">
        import init from './{PKG_DIR}/{OUT_NAME}.js';
        init();
    </script>
</body>
</html>
"#
    )
}

/// The manifest of an application named `name` that has none of its own.
#[must_use]
pub(crate) fn default_manifest(name: &str) -> String {
    let manifest = serde_json::json!({
        "name": name,
        "short_name": name,
        "start_url": ".",
        "display": "standalone",
        "background_color": "#FFFFFF",
        "theme_color": "#FFFFFF",
    });
    let mut text = serde_json::to_string_pretty(&manifest).expect("BUG: a JSON value serializes");
    text.push('\n');
    text
}

/// `text` as HTML text or a double-quoted attribute value.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_matches_its_golden() {
        insta::assert_snapshot!(render("Demo", &BaseHref::root()));
    }

    /// The page names its base, covers the display, links the manifest and
    /// imports the module relative to the base, holds no canvas and no
    /// script file of its own, and escapes the application's name.
    #[test]
    fn the_page_is_a_mount_point_and_a_loader() {
        let base = BaseHref::try_new("/apps/demo/").expect("a base path");
        let page = render("Tom & \"Jerry\" <3", &base);
        for (part, what) in [
            (r#"<base href="/apps/demo/">"#, "the base"),
            (
                "viewport-fit=cover",
                "the display covered, so the safe area reads",
            ),
            (
                r#"<link rel="manifest" href="manifest.json">"#,
                "the manifest",
            ),
            ("import init from './pkg/app.js';", "the module"),
            (
                "<title>Tom &amp; &quot;Jerry&quot; &lt;3</title>",
                "the escaped name",
            ),
        ] {
            assert!(page.contains(part), "{what}: {page}");
        }
        assert!(!page.contains("<canvas"), "the host makes its own root");
        assert!(!page.contains("src="), "no script file beside the module");
    }

    #[test]
    fn a_base_path_is_an_absolute_directory_with_nothing_to_escape() {
        for accepted in ["/", "/apps/demo/", "/a-b_c.d~e/"] {
            assert_eq!(
                BaseHref::try_new(accepted).map(|base| base.to_string()),
                Ok(accepted.to_owned())
            );
        }
        for refused in ["", "apps/", "/apps", "https://example.com/"] {
            assert!(
                matches!(
                    BaseHref::try_new(refused),
                    Err(InvalidBaseHref::NotADirectory(_))
                ),
                "{refused}"
            );
        }
        for refused in [
            "/a b/", "/a\"/", "/<x>/", "/a?b/", "/a#b/", "/a\\b/", "/a\n/",
        ] {
            assert!(
                matches!(
                    BaseHref::try_new(refused),
                    Err(InvalidBaseHref::Character(_))
                ),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn the_default_manifest_names_the_application() {
        let manifest: serde_json::Value =
            serde_json::from_str(&default_manifest("Demo")).expect("JSON");
        assert_eq!(manifest["name"], "Demo");
        assert_eq!(manifest["start_url"], ".");
    }
}
