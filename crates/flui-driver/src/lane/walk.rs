//! A bounded walk over an OS accessibility tree, apart from the API that serves it: the read
//! projected onto the wire's nodes and the search for one automation id, both within
//! [`MAX_NODES`] elements and [`MAX_DEPTH`] levels.
//!
//! A provider that does not answer in time fails the walk at once with a typed timeout: a hung app
//! never reads as a tree of empty defaults. A list read short of an element's children is marked
//! unread on that element and the read cut short, never a shorter list read as whole, whatever
//! cut it ([`Cut`]: a bound, the list gone, or one of its elements gone while read, which is left
//! out); and a property its provider does not offer reads as the field's default, which is the
//! provider's own answer. [`Cut`] is the rule of the two reads of a tree, this one ([`read`])
//! and the iOS Simulator's screen walk (`ios_simulator::accessibility::walk_screen`); the
//! searches ([`find_by`] here, the Simulator's screen search) keep their own record of a cut,
//! which their failures name.

use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::error::{ErrorCode, HandleKind, Retry};
use flui_protocol::tree::{ElementId, Node, Tree};

use crate::error::{Error, Result};

/// The most elements one walk visits.
pub const MAX_NODES: usize = 4096;

/// The deepest level one walk visits.
pub const MAX_DEPTH: usize = 64;

/// Why a walk read a list short of an element's children, the cause each reader of the cut acts
/// on: a read again cuts a [`Cut::Bound`] the same way and may heal the others, and only an
/// [`Cut::ElementGone`] leaves the list read to its end. Ordered from the least lasting to the
/// most, so the larger of two is the one that speaks for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Cut {
    /// One element of the list went away while it was read: it is left out, and the list was read
    /// on to its end, so it could have been the twin of one read.
    ElementGone,
    /// The list went away before it was read to its end: its parent while its children were
    /// listed, or the element a step to the next was asked from.
    ListGone,
    /// A bound stopped the list: the walk's budget or its depth.
    Bound,
}

impl Cut {
    /// Whether a read again cuts the list the same way, so no read again serves it.
    #[must_use]
    pub const fn repeats(self) -> bool {
        matches!(self, Self::Bound)
    }

    /// Whether an element went away: what a source that answers a gone element as a refusal is
    /// asked about.
    #[must_use]
    pub const fn is_gone(self) -> bool {
        !self.repeats()
    }

    /// `cut` and `other` as one: the more lasting of the two.
    #[must_use]
    pub fn with(cut: Option<Self>, other: Self) -> Option<Self> {
        Some(cut.map_or(other, |cut| cut.max(other)))
    }
}

/// Why a provider could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    /// It did not answer within the client's timeout.
    Timeout,
    /// The element went away: the one whose properties were read, or the one a step to a child or
    /// a sibling was asked of, whose list the walk then marks cut short.
    Gone,
    /// Anything else, for people.
    Failed(String),
}

/// An OS accessibility tree as the walk reads it.
pub trait Provider {
    /// One element.
    type Element;

    /// The first child of `element`, if it has one.
    ///
    /// # Errors
    ///
    /// When the provider cannot answer.
    fn first_child(
        &mut self,
        element: &Self::Element,
    ) -> Result<Option<Self::Element>, ProviderError>;

    /// The next sibling of `element`, if it has one.
    ///
    /// # Errors
    ///
    /// When the provider cannot answer.
    fn next_sibling(
        &mut self,
        element: &Self::Element,
    ) -> Result<Option<Self::Element>, ProviderError>;

    /// `element` as a node with the handle `id`.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Timeout`] when any property did not answer in time.
    fn node(&mut self, element: &Self::Element, id: ElementId) -> Result<Node, ProviderError>;
}

/// One element's property reads for a [`Provider::node`]: after the first read that times out or
/// finds the element gone, no further call is made, so a hung provider costs one timeout per
/// walk, not one per property. A property the provider does not offer reads as `None`.
#[derive(Debug, Default)]
pub struct Properties {
    failure: Option<ProviderError>,
}

impl Properties {
    /// No read made yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Make the read `call`, unless an earlier one timed out or found the element gone.
    pub fn read<T>(&mut self, call: impl FnOnce() -> Result<T, ProviderError>) -> Option<T> {
        if self.failure.is_some() {
            return None;
        }
        match call() {
            Ok(value) => Some(value),
            Err(failure @ (ProviderError::Timeout | ProviderError::Gone)) => {
                self.failure = Some(failure);
                None
            }
            Err(_) => None,
        }
    }

    /// `value`, the node the reads made, or the first timeout or disappearance among them.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Timeout`] or [`ProviderError::Gone`], the first a read met.
    pub fn finish<T>(self, value: T) -> Result<T, ProviderError> {
        match self.failure {
            Some(failure) => Err(failure),
            None => Ok(value),
        }
    }
}

fn timed_out(timeout: Duration) -> Error {
    Error::Timeout {
        awaited: crate::error::Awaited::Host,
        effect: None,
        message: format!(
            "the app's accessibility provider did not answer within {} ms: the app may be hung",
            timeout.as_millis()
        ),
        retry: ErrorCode::Timeout.default_retry(),
    }
}

fn failed(why: &str) -> Error {
    Error::Platform {
        effect: None,
        message: format!("the OS accessibility tree could not be read: {why}"),
        retry: Retry::Never,
    }
}

/// The control pattern `pattern` of the element `automation_id`, from its provider's `answer`,
/// `Ok(None)` when the element does not offer it.
///
/// # Errors
///
/// [`Error::ActionUnsupported`] when the element offers no such pattern, [`Error::Timeout`] when
/// its provider did not answer within `timeout`, [`Error::Gone`] when the element went away, and
/// [`Error::Platform`] when the provider failed otherwise.
pub fn pattern<T>(
    answer: Result<Option<T>, ProviderError>,
    automation_id: &str,
    pattern: &str,
    timeout: Duration,
) -> Result<T> {
    match answer {
        Ok(Some(found)) => Ok(found),
        Ok(None) => Err(Error::ActionUnsupported {
            message: format!("the element {automation_id:?} offers no {pattern} pattern"),
            retry: ErrorCode::ActionUnsupported.default_retry(),
        }),
        Err(ProviderError::Timeout) => Err(timed_out(timeout)),
        Err(ProviderError::Gone) => Err(element_gone(
            automation_id,
            "before it was asked for its pattern",
            None,
        )),
        Err(ProviderError::Failed(why)) => Err(Error::Platform {
            effect: None,
            message: format!(
                "the {pattern} pattern of the element {automation_id:?} could not be read: {why}"
            ),
            retry: Retry::Never,
        }),
    }
}

/// What invoking the element `automation_id` answered. The invoke may have reached the app
/// whenever its provider did not say it failed, so a timeout and an element or provider that went
/// during the call (`RPC_E_DISCONNECTED` among others) carry `may_have_run`.
///
/// # Errors
///
/// [`Error::Timeout`] with `may_have_run` when the provider did not answer within `timeout`,
/// [`Error::Gone`] with `may_have_run` when the element went away, and [`Error::Platform`] when
/// the invoke failed.
pub fn invoked(
    answer: Result<(), ProviderError>,
    automation_id: &str,
    timeout: Duration,
) -> Result<()> {
    match answer {
        Ok(()) => Ok(()),
        Err(ProviderError::Timeout) => Err(Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: Some(Effect::new(EffectKind::MayHaveRun)),
            message: format!(
                "the app's accessibility provider did not answer the invoke of {automation_id:?} \
                 within {} ms: read the tree for its effect",
                timeout.as_millis()
            ),
            retry: ErrorCode::Timeout.default_retry(),
        }),
        Err(ProviderError::Gone) => Err(element_gone(
            automation_id,
            "while it was invoked, which may have run first",
            Some(Effect::new(EffectKind::MayHaveRun)),
        )),
        Err(ProviderError::Failed(why)) => Err(Error::Platform {
            effect: None,
            message: format!("invoking the element {automation_id:?} failed: {why}"),
            retry: Retry::Never,
        }),
    }
}

fn element_gone(automation_id: &str, when: &str, effect: Option<Effect>) -> Error {
    Error::Gone {
        kind: Some(HandleKind::Element),
        handle: None,
        exit: None,
        effect,
        message: format!("the element {automation_id:?} went away {when}"),
        retry: ErrorCode::Gone.default_retry(),
    }
}

fn provider_error(error: ProviderError, timeout: Duration) -> Error {
    match error {
        ProviderError::Timeout => timed_out(timeout),
        ProviderError::Gone => failed("an element went away"),
        ProviderError::Failed(why) => failed(&why),
    }
}

/// The tree under `root`, projected: see the [module docs](self). `timeout` is the client's, which
/// a timeout names.
///
/// # Errors
///
/// [`Error::Timeout`] when the provider did not answer in time; [`Error::Platform`] when the root
/// itself could not be read.
pub fn read<P: Provider>(provider: &mut P, root: &P::Element, timeout: Duration) -> Result<Tree> {
    let mut walk = Walk {
        next_id: 1,
        cut: None,
        timeout,
    };
    match walk.visit(provider, root, 0)? {
        Some(node) => Ok(Tree::new(vec![node], walk.cut.is_some())),
        None => Err(failed("the window's element went away")),
    }
}

struct Walk {
    next_id: u64,
    /// The most lasting cut the walk met, which `Tree::truncated` reads.
    cut: Option<Cut>,
    timeout: Duration,
}

impl Walk {
    /// Record that `parent`'s list was read short by `cause`: the one write that marks the level
    /// and the walk together.
    fn cut(&mut self, parent: &mut Node, cause: Cut) {
        parent.children_unread = true;
        self.cut = Cut::with(self.cut, cause);
    }

    fn visit<P: Provider>(
        &mut self,
        provider: &mut P,
        element: &P::Element,
        depth: usize,
    ) -> Result<Option<Node>> {
        let id = ElementId::from_u64(self.next_id)
            .expect("BUG: element numbers start at one and stay under the node budget");
        self.next_id += 1;
        let mut node = match provider.node(element, id) {
            Ok(node) => node,
            Err(ProviderError::Gone) => return Ok(None),
            Err(error) => return Err(provider_error(error, self.timeout)),
        };
        let first = self.step(provider.first_child(element), &mut node)?;
        if depth >= MAX_DEPTH {
            if first.is_some() {
                self.cut(&mut node, Cut::Bound);
            }
            return Ok(Some(node));
        }
        let mut child = first;
        while let Some(current) = child {
            if usize::try_from(self.next_id).unwrap_or(usize::MAX) > MAX_NODES {
                self.cut(&mut node, Cut::Bound);
                break;
            }
            match self.visit(provider, &current, depth + 1)? {
                Some(read) => node.children.push(read),
                // Gone while its properties were read: left out, and its level read short, since
                // it could have been the twin of one read.
                None => self.cut(&mut node, Cut::ElementGone),
            }
            child = self.step(provider.next_sibling(&current), &mut node)?;
        }
        Ok(Some(node))
    }

    /// A step to `parent`'s next child: an element gone (`parent` itself, or the sibling the step
    /// was asked from) ends the level cut ([`Cut::ListGone`]), since no provider can say what
    /// followed; a timeout or a failure ends the walk.
    fn step<E>(
        &mut self,
        step: Result<Option<E>, ProviderError>,
        parent: &mut Node,
    ) -> Result<Option<E>> {
        match step {
            Ok(next) => Ok(next),
            Err(ProviderError::Gone) => {
                self.cut(parent, Cut::ListGone);
                Ok(None)
            }
            Err(error) => Err(provider_error(error, self.timeout)),
        }
    }
}

/// The first element under `root`, in document order, whose automation id is `automation_id`.
///
/// # Errors
///
/// As [`find_by`].
pub fn find<P: Provider>(
    provider: &mut P,
    root: P::Element,
    automation_id: &str,
    timeout: Duration,
) -> Result<P::Element> {
    find_by(
        provider,
        root,
        |node| node.automation_id.as_deref() == Some(automation_id),
        &format!("whose automation id is {automation_id:?}"),
        timeout,
    )
}

/// The first element under `root`, in document order, whose node `matches` accepts; `wanted`
/// says which, for people ("whose automation id is \"like\"").
///
/// # Errors
///
/// [`Error::NotFound`] when no element within the walk's budgets is accepted, saying whether the
/// search was cut short: by its budgets, or by an element that went away before all its
/// children were searched; [`Error::Timeout`] and [`Error::Platform`] as [`read`].
pub fn find_by<P: Provider>(
    provider: &mut P,
    root: P::Element,
    mut matches: impl FnMut(&Node) -> bool,
    wanted: &str,
    timeout: Duration,
) -> Result<P::Element> {
    let mut visited = 0_usize;
    let mut cut_short = false;
    // Whether an element went away (on a step, a parent or a sibling; or while its properties
    // were read), which cut a level short.
    let mut met_gone = false;
    // Depth first, children before siblings: each level holds the next element to visit there.
    // The root's own siblings (the other windows) are not the app's.
    let mut levels: Vec<Option<P::Element>> = vec![Some(root)];
    loop {
        let depth = levels.len();
        let Some(slot) = levels.last_mut() else {
            break;
        };
        let Some(element) = slot.take() else {
            levels.pop();
            continue;
        };
        if visited >= MAX_NODES {
            cut_short = true;
            break;
        }
        visited += 1;
        let id = ElementId::from_u64(u64::try_from(visited).unwrap_or(u64::MAX))
            .expect("BUG: a visit count starts at one");
        let node = match provider.node(&element, id) {
            Ok(node) => Some(node),
            // Gone while its properties were read: as a read leaves it out and reads its level
            // short (`Cut::ElementGone`), the search says an element went away.
            Err(ProviderError::Gone) => {
                met_gone = true;
                None
            }
            Err(error) => return Err(provider_error(error, timeout)),
        };
        if node.is_some_and(|node| matches(&node)) {
            return Ok(element);
        }
        if depth > 1 {
            *slot = search_step(provider.next_sibling(&element), &mut met_gone, timeout)?;
        }
        if depth > MAX_DEPTH {
            if search_step(provider.first_child(&element), &mut met_gone, timeout)?.is_some() {
                cut_short = true;
            }
            continue;
        }
        let child = search_step(provider.first_child(&element), &mut met_gone, timeout)?;
        if child.is_some() {
            levels.push(child);
        }
    }
    Err(Error::not_found(format!(
        "the OS accessibility tree shows no element {wanted}{}{}",
        if cut_short {
            format!(" within its first {MAX_NODES} elements and {MAX_DEPTH} levels")
        } else {
            String::new()
        },
        if met_gone {
            ", and an element went away before all its children were searched"
        } else {
            ""
        }
    )))
}

/// A step of a search: as [`Walk::step`], an element gone (a parent or a sibling) recorded in
/// `met_gone`, which the search's failure names.
fn search_step<E>(
    step: Result<Option<E>, ProviderError>,
    met_gone: &mut bool,
    timeout: Duration,
) -> Result<Option<E>> {
    match step {
        Ok(next) => Ok(next),
        Err(ProviderError::Gone) => {
            *met_gone = true;
            Ok(None)
        }
        Err(error) => Err(provider_error(error, timeout)),
    }
}
