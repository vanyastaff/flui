//! Owner-thread navigation intents, normalized route locations, and the
//! presentation-local slot for its outermost URL owner (ADR-0093 §5).

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;

use flui_foundation::IdMint;
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

/// Bytes a route segment percent-encodes: FLUI's route-path escaping set,
/// (C0 controls, space, `"`, `#`, `<`, `>`, `?`, `` ` ``, `{`, `}`), plus
/// `/` and `%` to preserve segment boundaries and escapes, and `\`, which
/// special URL schemes read as a separator. Non-ASCII bytes are encoded too.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'\\')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// A normalized, percent-encoded route location.
///
/// It starts with `/`, has no empty segment, has no trailing `/` except the
/// root's, and carries no `?` query or `#` fragment (those are a later step).
/// Each segment is stored in one canonical encoding, so comparing two
/// `RoutePath`s compares the locations they name: `/tag/a b`, `/tag/a%20b`
/// and `/tag/a%20b/` are the same path.
///
/// Escapes are decoded exactly once. A literal `+` stays `+`; it is not a form
/// query's space. Encoded `/`, `?`, `#` and backslashes remain data within one
/// segment, while malformed escapes and invalid UTF-8 are rejected. This is
/// route-location normalization, not whole-URL parsing: `.` and `..` remain
/// literal route values rather than resolving relative to another location.
///
/// ```
/// use flui_view::RoutePath;
///
/// let path = RoutePath::root().join("note").join(7);
/// assert_eq!(path.as_str(), "/note/7");
/// assert_eq!(RoutePath::parse("/note/7/")?, path);
/// # Ok::<(), flui_view::RouteParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoutePath(String);

impl RoutePath {
    /// The root location, `/`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// Validate and normalize a location.
    ///
    /// # Errors
    ///
    /// [`RouteParseError::Malformed`] when `location` is empty, does not start
    /// with `/`, has an empty segment (`//x`), carries a query or fragment, or
    /// holds an escape that is not `%` and two hex digits or does not decode to
    /// UTF-8.
    pub fn parse(location: &str) -> Result<Self, RouteParseError> {
        let malformed = |reason| RouteParseError::Malformed {
            location: location.to_owned(),
            reason,
        };
        if location.is_empty() {
            return Err(malformed("it is empty"));
        }
        let Some(rest) = location.strip_prefix('/') else {
            return Err(malformed("it does not start with `/`"));
        };
        if location.contains(['?', '#']) {
            return Err(malformed("a query or fragment is not supported"));
        }
        let mut path = Self::root();
        if rest.is_empty() {
            return Ok(path);
        }
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        for segment in rest.split('/') {
            if segment.is_empty() {
                return Err(malformed("it has an empty segment"));
            }
            if !escapes_are_well_formed(segment) {
                return Err(malformed("`%` is not followed by two hex digits"));
            }
            let decoded = percent_decode_str(segment)
                .decode_utf8()
                .map_err(|_| malformed("an escape does not decode to UTF-8"))?;
            path.push_encoded(&decoded);
        }
        Ok(path)
    }

    /// Append one segment, printed through `Display` and percent-encoded.
    ///
    /// A value that prints as the empty string appends nothing: a location has
    /// no empty segment, so a route field that can print empty does not round
    /// trip and needs a non-empty form of its own.
    #[must_use]
    pub fn join(mut self, segment: impl fmt::Display) -> Self {
        let segment = segment.to_string();
        if !segment.is_empty() {
            self.push_encoded(&segment);
        }
        self
    }

    /// The location as text, percent-encoded.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The decoded segments, first to last; the root has none.
    pub fn segments(&self) -> impl Iterator<Item = Cow<'_, str>> + '_ {
        self.0
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| percent_decode_str(segment).decode_utf8_lossy())
    }

    /// Every prefix of this path, root first and this path last: `/`, `/a`,
    /// `/a/b` for `/a/b`.
    pub fn prefixes(&self) -> impl Iterator<Item = RoutePath> + '_ {
        let ends = self
            .0
            .match_indices('/')
            .map(|(index, _)| index)
            .skip(1)
            .chain(std::iter::once(self.0.len()));
        std::iter::once(Self::root()).chain(
            ends.filter(|&end| end > 1)
                .map(|end| Self(self.0[..end].to_owned())),
        )
    }

    fn push_encoded(&mut self, decoded: &str) {
        if self.0.len() > 1 {
            self.0.push('/');
        }
        self.0.extend(utf8_percent_encode(decoded, SEGMENT));
    }
}

impl fmt::Display for RoutePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether every `%` in `segment` starts a `%XX` escape of two hex digits.
fn escapes_are_well_formed(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.iter().enumerate().all(|(index, &byte)| {
        byte != b'%'
            || bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
    })
}

/// Why a location did not produce a route.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteParseError {
    /// The text is not a route location at all.
    #[error("{location:?} is not a route location: {reason}")]
    Malformed {
        /// The text that was given.
        location: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The location is well formed, but no route matches it.
    #[error("no route matches {path}")]
    NoMatch {
        /// The location that matched nothing.
        path: RoutePath,
    },
    /// A route's pattern matches, but one segment does not parse as the field
    /// it fills.
    #[error("segment {segment:?} of {path} is not a valid `{field}`")]
    Param {
        /// The location being parsed.
        path: RoutePath,
        /// The field the segment was meant to fill.
        field: &'static str,
        /// The decoded segment that did not parse.
        segment: String,
    },
}

/// A navigation operation addressed to a presentation's URL owner.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationOp {
    /// Push a location above the current route stack.
    Push(RoutePath),
    /// Replace the current location without growing the route stack.
    Replace(RoutePath),
    /// Pop the current location.
    Pop,
    /// Navigate to a location, reconciling it with the current route stack.
    Go(RoutePath),
}

/// One owner-thread navigation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationIntent {
    /// The operation the URL owner should apply.
    pub op: NavigationOp,
}

impl NavigationIntent {
    /// Construct an intent for `op`.
    #[must_use]
    pub const fn new(op: NavigationOp) -> Self {
        Self { op }
    }
}

/// Why a navigation intent was not applied.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavigationRefused {
    /// The addressed presentation no longer exists.
    #[error("the addressed presentation no longer exists")]
    NoPresentation,
    /// The presentation has no registered URL owner.
    #[error("the presentation has no URL owner")]
    NoUrlOwner,
    /// The location is well formed, but no route matches it.
    #[error("no route matches {0}")]
    NoMatch(RoutePath),
    /// The URL owner is already committing another navigation.
    #[error("the URL owner is committing another navigation")]
    Busy,
}

/// The owner-thread authority that applies navigation intents for one
/// presentation.
///
/// Implementations are deliberately owner-local: no `Send` or `Sync` bound is
/// imposed, and an intent never becomes a cross-thread command.
pub trait UrlOwner: 'static {
    /// Apply `intent`, or refuse it without starting another navigation edit.
    ///
    /// # Errors
    ///
    /// [`NavigationRefused`] describes why the intent was not applied.
    fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused>;
}

struct RegisteredOwner {
    generation: NonZeroU64,
    owner: Rc<dyn UrlOwner>,
}

struct State {
    mint: IdMint<NonZeroU64>,
    owner: Option<RegisteredOwner>,
}

/// The single URL-owner slot belonging to one presentation.
///
/// Clones share the same owner-local state. The slot holds no process-global
/// registry and imposes no `Send` or `Sync` contract. A presentation installs
/// one through [`PresentationCapabilities`](crate::PresentationCapabilities);
/// its outermost router acquires it through
/// [`LifecycleContext::url_owner_slot`](crate::LifecycleContext::url_owner_slot).
#[derive(Clone)]
pub struct UrlOwnerSlot {
    state: Rc<RefCell<State>>,
}

impl UrlOwnerSlot {
    /// Register `owner` as the presentation's only active URL owner.
    ///
    /// Keep the returned registration alive for as long as the owner accepts
    /// intents. Dropping it unregisters only its own generation, after which
    /// another owner may register.
    ///
    /// # Errors
    ///
    /// [`UrlOwnerRegistrationError::Occupied`] when another registration is
    /// active, or [`UrlOwnerRegistrationError::Exhausted`] when this slot's
    /// never-reused identity space is exhausted.
    pub fn try_register(
        &self,
        owner: Rc<dyn UrlOwner>,
    ) -> Result<UrlOwnerRegistration, UrlOwnerRegistrationError> {
        let mut state = self.state.borrow_mut();
        if state.owner.is_some() {
            drop(state);
            return Err(UrlOwnerRegistrationError::Occupied);
        }
        let Ok(generation) = state.mint.next() else {
            drop(state);
            return Err(UrlOwnerRegistrationError::Exhausted);
        };
        state.owner = Some(RegisteredOwner { generation, owner });
        drop(state);
        Ok(UrlOwnerRegistration {
            state: Rc::clone(&self.state),
            generation,
        })
    }

    /// Clone the active owner, if one is registered.
    ///
    /// The `Rc` is cloned before the state borrow ends, so calling
    /// [`UrlOwner::navigate`] on the result never runs owner code under the
    /// slot's borrow.
    #[must_use]
    pub fn owner(&self) -> Option<Rc<dyn UrlOwner>> {
        let state = self.state.borrow();
        state
            .owner
            .as_ref()
            .map(|registered| Rc::clone(&registered.owner))
    }
}

impl Default for UrlOwnerSlot {
    fn default() -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                mint: IdMint::new(),
                owner: None,
            })),
        }
    }
}

impl fmt::Debug for UrlOwnerSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UrlOwnerSlot")
            .field("occupied", &self.state.borrow().owner.is_some())
            .finish()
    }
}

/// Why a URL owner could not be registered.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UrlOwnerRegistrationError {
    /// Another URL owner registration is active in this slot.
    #[error("the presentation's URL-owner slot is occupied")]
    Occupied,
    /// The slot has issued every registration identity it can represent.
    #[error("the presentation's URL-owner registration identities are exhausted")]
    Exhausted,
}

/// The RAII registration of one URL-owner generation.
///
/// Dropping the registration releases only that generation. A stale
/// registration cannot unregister a newer owner. The owner's `Rc` is removed
/// from the slot and dropped after the state borrow ends.
#[must_use = "dropping the registration unregisters its URL owner"]
pub struct UrlOwnerRegistration {
    state: Rc<RefCell<State>>,
    generation: NonZeroU64,
}

impl fmt::Debug for UrlOwnerRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UrlOwnerRegistration")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl Drop for UrlOwnerRegistration {
    fn drop(&mut self) {
        let retired = {
            let mut state = self.state.borrow_mut();
            if state
                .owner
                .as_ref()
                .is_some_and(|registered| registered.generation == self.generation)
            {
                state.owner.take().map(|registered| registered.owner)
            } else {
                None
            }
        };
        drop(retired);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn canonical_location(input: &str, expected: &str, name: &str) {
        let path = RoutePath::parse(input).expect("valid encoded location");
        assert_eq!(path.as_str(), expected);
        let segments: Vec<String> = path.segments().map(Cow::into_owned).collect();
        assert_eq!(segments, ["tag", name]);
        assert_eq!(RoutePath::root().join("tag").join(name), path);
        assert_eq!(
            path.prefixes()
                .map(|prefix| prefix.to_string())
                .collect::<Vec<_>>(),
            ["/", "/tag", expected],
        );
    }

    fn lowercase_escapes_preserve_segment_boundaries() {
        canonical_location("/tag/a%2fb/", "/tag/a%2Fb", "a/b");
    }

    fn percent_is_decoded_once() {
        canonical_location("/tag/%252f", "/tag/%252f", "%2f");
    }

    fn plus_is_literal_path_data() {
        canonical_location("/tag/a+b", "/tag/a+b", "a+b");
    }

    fn reserved_delimiters_are_segment_data() {
        canonical_location("/tag/%3f%23%25", "/tag/%3F%23%25", "?#%");
    }

    fn unicode_is_encoded_as_utf8_bytes() {
        canonical_location("/tag/雪💖", "/tag/%E9%9B%AA%F0%9F%92%96", "雪💖");
    }

    fn controls_and_backslashes_are_encoded() {
        canonical_location("/tag/\0\t\u{7f}\\", "/tag/%00%09%7F%5C", "\0\t\u{7f}\\");
    }

    fn readable_path_punctuation_is_preserved() {
        canonical_location(
            "/tag/-._~!$&'()*+,;=:@[]^|",
            "/tag/-._~!$&'()*+,;=:@[]^|",
            "-._~!$&'()*+,;=:@[]^|",
        );
    }

    fn dot_segments_are_literal_route_values() {
        canonical_location("/tag/%2e%2e", "/tag/..", "..");
    }

    fn malformed_location(input: &str) {
        assert!(
            matches!(
                RoutePath::parse(input),
                Err(RouteParseError::Malformed { location, .. }) if location == input
            ),
            "invalid location must be rejected without replacing data: {input:?}"
        );
    }

    fn missing_escape_digits_are_rejected() {
        malformed_location("/tag/%");
    }

    fn short_escape_is_rejected() {
        malformed_location("/tag/%0");
    }

    fn non_hex_escape_is_rejected() {
        malformed_location("/tag/%g0");
    }

    fn raw_query_is_rejected() {
        malformed_location("/tag/x?y");
    }

    fn raw_fragment_is_rejected() {
        malformed_location("/tag/x#y");
    }

    fn invalid_utf8_byte_is_rejected() {
        malformed_location("/tag/%FF");
    }

    fn overlong_utf8_is_rejected() {
        malformed_location("/tag/%C0%AF");
    }

    fn utf8_surrogate_is_rejected() {
        malformed_location("/tag/%ED%A0%80");
    }

    fn truncated_utf8_is_rejected() {
        malformed_location("/tag/%E2%82");
    }

    fn out_of_range_unicode_is_rejected() {
        malformed_location("/tag/%F4%90%80%80");
    }

    #[test]
    fn route_locations_preserve_encoded_segment_identity() {
        crate::table_test::run_table(
            "route location encoding",
            &[
                (
                    "lowercase escapes and separators",
                    lowercase_escapes_preserve_segment_boundaries,
                ),
                ("decode percent once", percent_is_decoded_once),
                ("literal plus", plus_is_literal_path_data),
                ("encoded delimiters", reserved_delimiters_are_segment_data),
                ("UTF-8 bytes", unicode_is_encoded_as_utf8_bytes),
                (
                    "controls and backslash",
                    controls_and_backslashes_are_encoded,
                ),
                (
                    "readable punctuation",
                    readable_path_punctuation_is_preserved,
                ),
                (
                    "literal dot segments",
                    dot_segments_are_literal_route_values,
                ),
                ("missing digits", missing_escape_digits_are_rejected),
                ("short escape", short_escape_is_rejected),
                ("non-hex escape", non_hex_escape_is_rejected),
                ("raw query", raw_query_is_rejected),
                ("raw fragment", raw_fragment_is_rejected),
                ("invalid UTF-8 byte", invalid_utf8_byte_is_rejected),
                ("overlong UTF-8", overlong_utf8_is_rejected),
                ("UTF-8 surrogate", utf8_surrogate_is_rejected),
                ("truncated UTF-8", truncated_utf8_is_rejected),
                ("out-of-range Unicode", out_of_range_unicode_is_rejected),
            ],
        );
    }

    struct TestOwner;

    impl UrlOwner for TestOwner {
        fn navigate(&self, _intent: NavigationIntent) -> Result<(), NavigationRefused> {
            Ok(())
        }
    }

    fn owner() -> Rc<dyn UrlOwner> {
        Rc::new(TestOwner)
    }

    fn registration_is_exclusive_and_drop_allows_reregistration() {
        let slot = UrlOwnerSlot::default();
        let first = slot
            .try_register(owner())
            .expect("empty slot accepts owner");
        assert!(slot.owner().is_some());
        assert!(matches!(
            slot.try_register(owner()),
            Err(UrlOwnerRegistrationError::Occupied)
        ));
        drop(first);
        assert!(slot.owner().is_none());
        let second = slot
            .try_register(owner())
            .expect("dropped owner released slot");
        assert!(slot.owner().is_some());
        drop(second);
        assert!(slot.owner().is_none());
    }

    fn stale_registration_does_not_remove_a_new_owner() {
        let slot = UrlOwnerSlot::default();
        let first = slot.try_register(owner()).expect("first registration");
        let stale = UrlOwnerRegistration {
            state: Rc::clone(&first.state),
            generation: first.generation,
        };
        drop(first);
        let _second = slot.try_register(owner()).expect("second registration");
        drop(stale);
        assert!(slot.owner().is_some());
    }

    fn exhausted_mint_refuses_registration() {
        let slot = UrlOwnerSlot::default();
        slot.state.borrow_mut().mint = IdMint::starting_after(u64::MAX);
        assert!(matches!(
            slot.try_register(owner()),
            Err(UrlOwnerRegistrationError::Exhausted)
        ));
        assert!(slot.owner().is_none());
    }

    struct ReentrantDropOwner {
        slot: UrlOwnerSlot,
        dropped: Rc<Cell<bool>>,
    }

    impl UrlOwner for ReentrantDropOwner {
        fn navigate(&self, _intent: NavigationIntent) -> Result<(), NavigationRefused> {
            Ok(())
        }
    }

    impl Drop for ReentrantDropOwner {
        fn drop(&mut self) {
            self.dropped.set(true);
            assert!(self.slot.owner().is_none());
            let replacement = self
                .slot
                .try_register(owner())
                .expect("owner drops after the slot borrow ends");
            drop(replacement);
        }
    }

    fn owner_drop_runs_outside_the_slot_borrow() {
        let slot = UrlOwnerSlot::default();
        let dropped = Rc::new(Cell::new(false));
        let owner: Rc<dyn UrlOwner> = Rc::new(ReentrantDropOwner {
            slot: slot.clone(),
            dropped: Rc::clone(&dropped),
        });
        let registration = slot.try_register(owner).expect("registration");
        drop(registration);
        assert!(dropped.get());
        assert!(slot.owner().is_none());
    }

    #[test]
    fn url_owner_slot_contract_matrix() {
        crate::table_test::run_table(
            "URL owner slot",
            &[
                (
                    "exclusive registration and reregistration",
                    registration_is_exclusive_and_drop_allows_reregistration,
                ),
                (
                    "stale identity",
                    stale_registration_does_not_remove_a_new_owner,
                ),
                ("exhaustion", exhausted_mint_refuses_registration),
                (
                    "owner drop outside borrow",
                    owner_drop_runs_outside_the_slot_borrow,
                ),
            ],
        );
    }
}
