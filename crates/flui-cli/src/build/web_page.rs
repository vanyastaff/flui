//! The page a web build serves, written whole on every build (ADR-0255
//! decision 16): a mount point and a loader. The page holds no canvas; the
//! host makes its own root in the body when the module starts. It asks to
//! cover the whole display (`viewport-fit=cover`), so the host reads the
//! safe area from `env(safe-area-inset-*)`; its `<base href>` resolves the
//! module, the stylesheet and the manifest from the deployment's base path
//! whatever address the page is served at, so an application can be hosted
//! under a sub-path and answer a deep link with the same page (which
//! `flui run`'s server does, `serve.rs`).
//!
//! The page holds no inline script and no inline style: it links
//! [`STYLESHEET`] and loads [`BOOTSTRAP`], both written beside it, so it runs
//! under a Content Security Policy of `script-src 'self'; style-src 'self'`.
//!
//! A project that needs its own page names a template (`[web] page` in
//! `flui.toml`): the build substitutes the placeholders of [`Placeholder`]
//! into it and refuses a template without the module's placeholder.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The path a web build is served from: absolute, starting and ending with
/// `/` (`/`, `/apps/demo/`), each segment between non-empty, not `.` or
/// `..`, and made of the characters a URL path carries unescaped
/// (`A-Z a-z 0-9 - . _ ~`). So it cannot name another site (`//host/`),
/// climb out of one (`/../`), or carry an escape a server decodes into one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct BaseHref(String);

/// Why a base path was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InvalidBaseHref {
    /// It does not start and end with `/`.
    #[error("`{0}` must start and end with `/` (`/`, `/apps/demo/`)")]
    NotADirectory(String),
    /// A segment is empty (`//`, which a browser reads as another site) or
    /// a dot segment (`.`, `..`).
    #[error("`{0}` has an empty, `.` or `..` segment")]
    Segment(String),
    /// It holds a character other than `A-Z a-z 0-9 - . _ ~` and `/`: a
    /// space, a quote, a query, a fragment, a backslash, an escape (`%`) or a
    /// control character.
    #[error("`{0}` holds a character a base path cannot (only `A-Z a-z 0-9 - . _ ~` and `/`)")]
    Character(String),
}

impl BaseHref {
    /// The site's root.
    pub(crate) fn root() -> Self {
        Self("/".to_owned())
    }

    /// `path` as a base path.
    ///
    /// # Errors
    /// [`InvalidBaseHref`] when `path` does not start and end with `/`, has
    /// an empty or dot segment, or holds a character a base path cannot.
    pub(crate) fn try_new(path: impl Into<String>) -> Result<Self, InvalidBaseHref> {
        let path = path.into();
        if !path.starts_with('/') || !path.ends_with('/') {
            return Err(InvalidBaseHref::NotADirectory(path));
        }
        let unescaped = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
        if !path.chars().all(|c| c == '/' || unescaped(c)) {
            return Err(InvalidBaseHref::Character(path));
        }
        if path.len() > 1
            && path[1..path.len() - 1]
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(InvalidBaseHref::Segment(path));
        }
        Ok(Self(path))
    }

    /// The path, starting and ending with `/`.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
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
/// The module's name: `wasm-bindgen --out-name` writes `<PKG_DIR>/<OUT_NAME>.js`.
pub(crate) const OUT_NAME: &str = "app";
/// The web app manifest the page links, relative to its base.
pub(crate) const MANIFEST: &str = "manifest.json";
/// The module the page loads, which starts the application.
pub(crate) const BOOTSTRAP: &str = "flui.js";
/// The stylesheet the page links.
pub(crate) const STYLESHEET: &str = "flui.css";

/// A colour for the browser's chrome (`theme-color`, the manifest's
/// `theme_color`): a hex colour (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`)
/// or a CSS named colour (letters only), checked when `flui.toml` loads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct ThemeColor(String);

/// Why a theme colour was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a hex colour (`#1a73e8`) or a CSS colour name (`teal`)")]
pub(crate) struct InvalidThemeColor(String);

impl ThemeColor {
    /// `colour` as a theme colour.
    ///
    /// # Errors
    /// [`InvalidThemeColor`] when it is neither a hex colour nor a name.
    pub(crate) fn try_new(colour: impl Into<String>) -> Result<Self, InvalidThemeColor> {
        let colour = colour.into();
        let hex = colour.strip_prefix('#').is_some_and(|digits| {
            matches!(digits.len(), 3 | 4 | 6 | 8) && digits.bytes().all(|b| b.is_ascii_hexdigit())
        });
        let named =
            (1..=32).contains(&colour.len()) && colour.bytes().all(|b| b.is_ascii_alphabetic());
        if hex || named {
            Ok(Self(colour))
        } else {
            Err(InvalidThemeColor(colour))
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ThemeColor {
    type Error = InvalidThemeColor;

    fn try_from(colour: String) -> Result<Self, Self::Error> {
        Self::try_new(colour)
    }
}

impl From<ThemeColor> for String {
    fn from(colour: ThemeColor) -> Self {
        colour.0
    }
}

/// What a web build states about the application in its page and manifest:
/// `flui.toml`'s `[app] name` and its `[web]` table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct WebPage {
    /// The application's name; `None` leaves it to the package the build
    /// compiled.
    pub(crate) title: Option<String>,
    /// `[web] description`.
    pub(crate) description: Option<String>,
    /// `[web] theme_color`.
    pub(crate) theme_color: Option<ThemeColor>,
    /// `[web] base_href`.
    pub(crate) base_href: BaseHref,
    /// `[web] page`: the project's page template, relative to the project.
    pub(crate) template: Option<std::path::PathBuf>,
    /// The directory of the `flui.toml` this was read from, which `template`,
    /// a kept page and the project's manifest are resolved against; `None`
    /// for a build with no `flui.toml`.
    pub(crate) project: Option<std::path::PathBuf>,
}

/// A placeholder a project's page template holds, which the build replaces
/// in one pass, never inside text it inserted: an element the page needs, or
/// one of `flui.toml`'s values, escaped for HTML text and attribute values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placeholder {
    /// `{{flui_base}}`: the `<base href>`.
    Base,
    /// `{{flui_manifest}}`: the manifest link.
    Manifest,
    /// `{{flui_stylesheet}}`: the stylesheet link.
    Stylesheet,
    /// `{{flui_module}}`: the script that starts the application. A template
    /// without it is refused; the build checks that it is there, not that it
    /// is where a script runs (inside a comment, the application never
    /// starts).
    Module,
    /// `{{flui_title}}`: the application's name.
    Title,
    /// `{{flui_description}}`: `[web] description`, empty when absent.
    Description,
    /// `{{flui_theme_color}}`: `[web] theme_color`, empty when absent.
    ThemeColor,
}

impl Placeholder {
    const ALL: [Self; 7] = [
        Self::Base,
        Self::Manifest,
        Self::Stylesheet,
        Self::Module,
        Self::Title,
        Self::Description,
        Self::ThemeColor,
    ];

    /// The text the template holds.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Base => "{{flui_base}}",
            Self::Manifest => "{{flui_manifest}}",
            Self::Stylesheet => "{{flui_stylesheet}}",
            Self::Module => "{{flui_module}}",
            Self::Title => "{{flui_title}}",
            Self::Description => "{{flui_description}}",
            Self::ThemeColor => "{{flui_theme_color}}",
        }
    }

    /// What the build puts in its place for the application named `title`.
    fn value(self, title: &str, page: &WebPage) -> String {
        match self {
            Self::Base => format!(r#"<base href="{}">"#, page.base_href),
            Self::Manifest => format!(r#"<link rel="manifest" href="{MANIFEST}">"#),
            Self::Stylesheet => format!(r#"<link rel="stylesheet" href="{STYLESHEET}">"#),
            Self::Module => format!(r#"<script type="module" src="{BOOTSTRAP}"></script>"#),
            Self::Title => escape(title),
            Self::Description => escape(page.description.as_deref().unwrap_or_default()),
            Self::ThemeColor => escape(page.theme_color.as_ref().map_or("", ThemeColor::as_str)),
        }
    }
}

/// Why a project's page template was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InvalidTemplate {
    /// It has no `{{flui_module}}`, so the page would never start the
    /// application.
    #[error("the page template has no `{{{{flui_module}}}}`, so it would never start the app")]
    NoModule,
}

/// The page `template` makes for the application named `title`: each
/// placeholder replaced in one pass, left to right, never inside a value it
/// put in, so a value that holds a placeholder's text stays text.
///
/// # Errors
/// [`InvalidTemplate::NoModule`] when the template has no
/// `{{flui_module}}`.
pub(crate) fn render_template(
    template: &str,
    title: &str,
    page: &WebPage,
) -> Result<String, InvalidTemplate> {
    if !template.contains(Placeholder::Module.token()) {
        return Err(InvalidTemplate::NoModule);
    }
    let mut page_text = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some(start) = rest.find("{{flui_") {
        let (before, from) = rest.split_at(start);
        page_text.push_str(before);
        if let Some(placeholder) = Placeholder::ALL
            .into_iter()
            .find(|placeholder| from.starts_with(placeholder.token()))
        {
            page_text.push_str(&placeholder.value(title, page));
            rest = &from[placeholder.token().len()..];
        } else {
            // Not a placeholder: kept as written.
            page_text.push_str("{{flui_");
            rest = &from["{{flui_".len()..];
        }
    }
    page_text.push_str(rest);
    Ok(page_text)
}

/// The page of the application named `title`, from `page`'s description,
/// theme colour and base path: the fixed skeleton's placeholders replaced in
/// one pass ([`render_template`]).
#[must_use]
pub(crate) fn render(title: &str, page: &WebPage) -> String {
    let description = if page.description.is_some() {
        "\n    <meta name=\"description\" content=\"{{flui_description}}\">"
    } else {
        ""
    };
    let theme = if page.theme_color.is_some() {
        "\n    <meta name=\"theme-color\" content=\"{{flui_theme_color}}\">"
    } else {
        ""
    };
    let skeleton = format!(
        r#"<!DOCTYPE html>
<!-- Written by `flui build` on every build; edits here are lost. -->
<html lang="en">
<head>
    <meta charset="UTF-8">
    {{{{flui_base}}}}
    <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
    <meta name="mobile-web-app-capable" content="yes">
    <meta name="apple-mobile-web-app-title" content="{{{{flui_title}}}}">
    <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">{description}{theme}
    <title>{{{{flui_title}}}}</title>
    {{{{flui_manifest}}}}
    {{{{flui_stylesheet}}}}
</head>
<body>
    {{{{flui_module}}}}
</body>
</html>
"#
    );
    render_template(&skeleton, title, page)
        .expect("BUG: the generated page holds the module's placeholder")
}

/// The module the page loads: it imports the application's module and
/// starts it.
#[must_use]
pub(crate) fn bootstrap() -> String {
    format!("import init from './{PKG_DIR}/{OUT_NAME}.js';\ninit();\n")
}

/// The page's stylesheet: no margin and no scrolling, so the host's root
/// covers the viewport, and no pull-to-refresh or bounce past the content.
#[must_use]
pub(crate) fn stylesheet() -> String {
    "html, body { margin: 0; height: 100%; overflow: hidden; overscroll-behavior: none; }\n"
        .to_owned()
}

/// The web app manifest of the application named `name`, with `page`'s
/// description and theme colour: what a build writes when the project keeps
/// none and what `flui platform add web` scaffolds.
#[must_use]
pub(crate) fn manifest(name: &str, page: &WebPage) -> String {
    let mut manifest = serde_json::json!({
        "name": name,
        "short_name": name,
        "start_url": ".",
        "display": "standalone",
        "background_color": "#FFFFFF",
        "theme_color": page.theme_color.as_ref().map_or("#FFFFFF", ThemeColor::as_str),
    });
    if let Some(description) = &page.description {
        manifest["description"] = serde_json::Value::from(description.as_str());
    }
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

    fn page(base: &str) -> WebPage {
        WebPage {
            base_href: BaseHref::try_new(base).expect("a base path"),
            ..WebPage::default()
        }
    }

    #[test]
    fn the_page_matches_its_golden() {
        insta::assert_snapshot!(render("Demo", &page("/")));
    }

    /// The page names its base, covers the display, links the manifest and
    /// the stylesheet and loads the bootstrap module relative to the base,
    /// holds no canvas, no inline script and no inline style, and escapes
    /// the application's name, description and theme colour.
    #[test]
    fn the_page_is_a_mount_point_and_a_loader_with_nothing_inline() {
        let mut described = page("/apps/demo/");
        described.description = Some("Cats & \"dogs\"".to_owned());
        described.theme_color = Some(ThemeColor::try_new("#1a73e8").expect("a colour"));
        let html = render("Tom & \"Jerry\" <3", &described);
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
            (
                r#"<link rel="stylesheet" href="flui.css">"#,
                "the stylesheet",
            ),
            (
                r#"<script type="module" src="flui.js"></script>"#,
                "the module",
            ),
            (
                "<title>Tom &amp; &quot;Jerry&quot; &lt;3</title>",
                "the escaped name",
            ),
            (
                r#"<meta name="description" content="Cats &amp; &quot;dogs&quot;">"#,
                "the escaped description",
            ),
            (
                r##"<meta name="theme-color" content="#1a73e8">"##,
                "the theme colour",
            ),
        ] {
            assert!(html.contains(part), "{what}: {html}");
        }
        assert!(!html.contains("<canvas"), "the host makes its own root");
        assert!(!html.contains("<style"), "no inline style");
        assert!(
            !html.contains("<script>") && !html.contains("<script type=\"module\">"),
            "no inline script"
        );
        assert!(bootstrap().contains("import init from './pkg/app.js';"));
        assert!(stylesheet().contains("overscroll-behavior: none"));
    }

    #[test]
    fn a_template_gets_each_placeholder_and_needs_the_module() {
        let template = "<head>{{flui_base}}{{flui_manifest}}{{flui_stylesheet}}\
                        <title>{{flui_title}}</title>{{flui_other}}</head>\
                        <body data-d=\"{{flui_description}}\" data-t=\"{{flui_theme_color}}\">\
                        {{flui_module}}</body>";
        let mut described = page("/x/");
        described.description = Some("A <b>".to_owned());
        described.theme_color = Some(ThemeColor::try_new("teal").expect("a colour"));
        let html = render_template(template, "T&T", &described).expect("a template");
        assert_eq!(
            html,
            "<head><base href=\"/x/\"><link rel=\"manifest\" href=\"manifest.json\">\
             <link rel=\"stylesheet\" href=\"flui.css\"><title>T&amp;T</title>{{flui_other}}</head>\
             <body data-d=\"A &lt;b&gt;\" data-t=\"teal\">\
             <script type=\"module\" src=\"flui.js\"></script></body>"
        );
        assert_eq!(
            render_template("<body></body>", "T", &described),
            Err(InvalidTemplate::NoModule)
        );
    }

    /// A value of `flui.toml` that holds a placeholder's text stays text: the
    /// page has its one module script and nothing breaks out of an attribute.
    #[test]
    fn a_value_holding_a_placeholder_stays_text() {
        for placeholder in Placeholder::ALL {
            let token = placeholder.token();
            let mut described = page("/");
            described.description = Some(token.to_owned());
            let html = render(token, &described);
            assert_eq!(
                html.matches("<script").count(),
                1,
                "{token} in the name and description: {html}"
            );
            assert_eq!(html.matches("<base ").count(), 1, "{token}: {html}");
            assert!(
                html.contains(&format!("content=\"{token}\"")),
                "{token} as text: {html}"
            );
        }
    }

    #[test]
    fn a_theme_colour_is_a_hex_colour_or_a_name() {
        for accepted in [
            "#fff",
            "#ffff",
            "#1a73e8",
            "#1a73e8cc",
            "teal",
            "RebeccaPurple",
        ] {
            assert!(ThemeColor::try_new(accepted).is_ok(), "{accepted}");
        }
        for refused in [
            "",
            "#",
            "#12",
            "#12345",
            "#ggg",
            "1a73e8",
            "red;",
            "rgb(1,2,3)",
            "#123\">",
            "a b",
        ] {
            assert!(ThemeColor::try_new(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn a_base_path_is_an_absolute_path_of_plain_segments() {
        for accepted in ["/", "/apps/demo/", "/a-b_c.d~e/", "/v1.2/"] {
            assert_eq!(
                BaseHref::try_new(accepted).map(|base| base.to_string()),
                Ok(accepted.to_owned())
            );
        }
        // Each refused path and the variant its refusal is.
        let cases = [
            ("", "NotADirectory"),
            ("apps/", "NotADirectory"),
            ("/apps", "NotADirectory"),
            ("https://evil.example/", "NotADirectory"),
            ("//evil.example/", "Segment"),
            ("//", "Segment"),
            ("/../", "Segment"),
            ("/./", "Segment"),
            ("/a/../b/", "Segment"),
            ("/a//b/", "Segment"),
            ("/%2e%2e/", "Character"),
            ("/a%2fb/", "Character"),
            ("/a%5Cb/", "Character"),
            ("/a\\b/", "Character"),
            ("/a b/", "Character"),
            ("/a\"/", "Character"),
            ("/<x>/", "Character"),
            ("/a?b/", "Character"),
            ("/a#b/", "Character"),
            ("/a\n/", "Character"),
        ];
        for (refused, variant) in cases {
            let error = BaseHref::try_new(refused).expect_err(refused);
            assert!(
                format!("{error:?}").starts_with(variant),
                "{refused:?}: {error:?}"
            );
        }
    }

    #[test]
    fn the_manifest_names_the_application_and_escapes_what_it_is_given() {
        let mut described = page("/");
        described.description = Some("A \"quoted\" app".to_owned());
        described.theme_color = Some(ThemeColor::try_new("#123456").expect("a colour"));
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest("Demo \"x\"", &described)).expect("JSON");
        assert_eq!(manifest["name"], "Demo \"x\"");
        assert_eq!(manifest["description"], "A \"quoted\" app");
        assert_eq!(manifest["theme_color"], "#123456");
        assert_eq!(manifest["start_url"], ".");
    }
}
