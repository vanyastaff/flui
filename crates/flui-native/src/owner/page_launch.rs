//! A page's launch values (ADR-0288 §7): the string properties of the object the launcher's
//! preload script installs as [`PAGE_LAUNCH_GLOBAL`], each named by a [`LaunchKey`]'s dotted
//! spelling.
//!
//! The web platform takes them once, as it starts: it reads the object's properties within
//! the bounds below, deletes the global whatever it found, and keeps the copy this module
//! checks. Every value is untrusted text, so each bound is checked before a property is
//! copied, and nothing read is ever traced: a value may be the driver's token.

use std::collections::BTreeMap;
use std::fmt;

use flui_platform::{LaunchKey, MAX_LAUNCH_KEY_LEN, PAGE_LAUNCH_GLOBAL};

/// The most properties a page's launch object has.
pub(crate) const MAX_PAGE_LAUNCH_VALUES: usize = 32;

/// The longest value a page's launch object holds, in UTF-16 units as the page counts a
/// string's length: checked before the value is copied.
pub(crate) const MAX_PAGE_LAUNCH_VALUE_LEN: usize = 4096;

/// The longest key the page's launch object is read with, in UTF-16 units, before it is
/// copied: a longer one keeps no [`LaunchKey`] rule.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(dead_code, reason = "the page's reader bounds its copies with it")
)]
pub(crate) const MAX_PAGE_LAUNCH_KEY_UNITS: usize = MAX_LAUNCH_KEY_LEN;

/// A property of the launch object, as the page read it within the bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageLaunchProperty {
    /// A string value, copied.
    Text(String),
    /// A string longer than [`MAX_PAGE_LAUNCH_VALUE_LEN`], left uncopied.
    TooLong,
    /// Anything but a string.
    NotText,
}

/// Why a page's launch object was refused. Names a key only when it is a launch key, never
/// what a value holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageLaunchError {
    /// The global is something other than a plain object (`null`, an array, a string, …).
    NotAnObject,
    /// The object has more than [`MAX_PAGE_LAUNCH_VALUES`] properties.
    TooMany {
        /// How many it has.
        count: usize,
    },
    /// A property's name breaks [`LaunchKey`]'s rules.
    NotAKey,
    /// The property `key` is not a string.
    NotText {
        /// The property's name, a launch key.
        key: String,
    },
    /// The property `key` is longer than [`MAX_PAGE_LAUNCH_VALUE_LEN`].
    TooLong {
        /// The property's name, a launch key.
        key: String,
    },
}

impl fmt::Display for PageLaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => write!(f, "`{PAGE_LAUNCH_GLOBAL}` is not a plain object"),
            Self::TooMany { count } => write!(
                f,
                "`{PAGE_LAUNCH_GLOBAL}` has {count} properties; it holds at most \
                 {MAX_PAGE_LAUNCH_VALUES}"
            ),
            Self::NotAKey => write!(
                f,
                "`{PAGE_LAUNCH_GLOBAL}` has a property whose name is not a launch key"
            ),
            Self::NotText { key } => {
                write!(f, "`{PAGE_LAUNCH_GLOBAL}[\"{key}\"]` is not a string")
            }
            Self::TooLong { key } => write!(
                f,
                "`{PAGE_LAUNCH_GLOBAL}[\"{key}\"]` is longer than {MAX_PAGE_LAUNCH_VALUE_LEN} \
                 characters"
            ),
        }
    }
}

impl std::error::Error for PageLaunchError {}

/// The launch values a page was started with: empty when it was given no launch object.
#[derive(Default, Clone, PartialEq, Eq)]
pub(crate) struct PageLaunch {
    values: BTreeMap<String, String>,
}

impl PageLaunch {
    /// Whether an object with `count` properties may be read at all: checked before any is.
    ///
    /// # Errors
    ///
    /// [`PageLaunchError::TooMany`] past [`MAX_PAGE_LAUNCH_VALUES`].
    pub(crate) fn check_count(count: usize) -> Result<(), PageLaunchError> {
        if count > MAX_PAGE_LAUNCH_VALUES {
            return Err(PageLaunchError::TooMany { count });
        }
        Ok(())
    }

    /// The launch the object's `properties` give. A key is `None` when the page found it
    /// longer than [`MAX_PAGE_LAUNCH_KEY_UNITS`] and left it uncopied.
    ///
    /// # Errors
    ///
    /// The first property that is not a launch key's string within its bounds, or more
    /// properties than [`MAX_PAGE_LAUNCH_VALUES`].
    pub(crate) fn try_from_properties(
        properties: impl IntoIterator<Item = (Option<String>, PageLaunchProperty)>,
    ) -> Result<Self, PageLaunchError> {
        let mut values = BTreeMap::new();
        for (count, (key, value)) in properties.into_iter().enumerate() {
            Self::check_count(count + 1)?;
            let key = key
                .filter(|key| LaunchKey::is_spelling(key))
                .ok_or(PageLaunchError::NotAKey)?;
            match value {
                PageLaunchProperty::Text(text)
                    if text.encode_utf16().count() <= MAX_PAGE_LAUNCH_VALUE_LEN =>
                {
                    values.insert(key, text);
                }
                PageLaunchProperty::Text(_) | PageLaunchProperty::TooLong => {
                    return Err(PageLaunchError::TooLong { key });
                }
                PageLaunchProperty::NotText => return Err(PageLaunchError::NotText { key }),
            }
        }
        Ok(Self { values })
    }

    /// The value launched under `key`; `None` when the launch did not give it or gave it
    /// empty.
    pub(crate) fn value(&self, key: LaunchKey) -> Option<String> {
        self.values
            .get(key.as_str())
            .filter(|value| !value.is_empty())
            .cloned()
    }
}

impl fmt::Debug for PageLaunch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The keys only: a value may be the driver's token.
        f.debug_struct("PageLaunch")
            .field("keys", &self.values.keys().collect::<Vec<&String>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: LaunchKey = match LaunchKey::try_new("flui.agent.token") {
        Some(key) => key,
        None => panic!("a valid key"),
    };
    const ENDPOINT: LaunchKey = match LaunchKey::try_new("flui.agent.endpoint") {
        Some(key) => key,
        None => panic!("a valid key"),
    };

    /// The properties a page read, in order.
    type Properties = Vec<(Option<String>, PageLaunchProperty)>;

    fn text(key: &str, value: &str) -> (Option<String>, PageLaunchProperty) {
        (
            Some(key.to_owned()),
            PageLaunchProperty::Text(value.to_owned()),
        )
    }

    #[test]
    fn a_page_launch_is_read_by_its_dotted_keys_and_an_empty_value_is_none() {
        let launch = PageLaunch::try_from_properties([
            text("flui.agent.endpoint", "__flui_agent"),
            text("flui.agent.token", ""),
            text("other.key", "kept"),
        ])
        .expect("a well-formed launch");
        assert_eq!(launch.value(ENDPOINT).as_deref(), Some("__flui_agent"));
        assert_eq!(launch.value(TOKEN), None, "an empty value is no value");
        assert_eq!(PageLaunch::default().value(ENDPOINT), None);
        assert!(
            !format!("{launch:?}").contains("__flui_agent"),
            "a value is never printed: {launch:?}"
        );
    }

    #[test]
    fn a_malformed_page_launch_is_refused_naming_only_a_launch_key() {
        let longest = "é".repeat(MAX_PAGE_LAUNCH_VALUE_LEN);
        let accepted = PageLaunch::try_from_properties([text("flui.agent.token", &longest)])
            .expect("a value at the bound, counted in UTF-16 units");
        assert_eq!(accepted.value(TOKEN), Some(longest.clone()));

        let too_many: Properties = (0..=MAX_PAGE_LAUNCH_VALUES)
            .map(|index| text(&format!("key{index}"), "v"))
            .collect();
        let cases: [(Properties, PageLaunchError); 6] = [
            (
                vec![text("flui.agent.token", &format!("{longest}a"))],
                PageLaunchError::TooLong {
                    key: "flui.agent.token".to_owned(),
                },
            ),
            (
                vec![(
                    Some("flui.agent.token".to_owned()),
                    PageLaunchProperty::TooLong,
                )],
                PageLaunchError::TooLong {
                    key: "flui.agent.token".to_owned(),
                },
            ),
            (
                vec![(
                    Some("flui.agent.token".to_owned()),
                    PageLaunchProperty::NotText,
                )],
                PageLaunchError::NotText {
                    key: "flui.agent.token".to_owned(),
                },
            ),
            (
                vec![text("FLUI_AGENT_TOKEN", "v")],
                PageLaunchError::NotAKey,
            ),
            (
                vec![(None, PageLaunchProperty::NotText)],
                PageLaunchError::NotAKey,
            ),
            (
                too_many,
                PageLaunchError::TooMany {
                    count: MAX_PAGE_LAUNCH_VALUES + 1,
                },
            ),
        ];
        for (properties, expected) in cases {
            let refused = PageLaunch::try_from_properties(properties)
                .expect_err("a malformed launch is refused");
            assert_eq!(refused, expected);
            assert!(
                !refused.to_string().contains("FLUI_AGENT_TOKEN"),
                "a name that is no key is not echoed: {refused}"
            );
        }
        assert_eq!(
            PageLaunch::check_count(MAX_PAGE_LAUNCH_VALUES + 1),
            Err(PageLaunchError::TooMany {
                count: MAX_PAGE_LAUNCH_VALUES + 1
            })
        );
        assert_eq!(PageLaunch::check_count(MAX_PAGE_LAUNCH_VALUES), Ok(()));
        assert_eq!(
            PageLaunchError::NotAnObject.to_string(),
            "`__FLUI_LAUNCH__` is not a plain object",
            "the refusal names the global the launcher installs"
        );
    }
}
