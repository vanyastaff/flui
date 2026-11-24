//! A bounded walk over an OS accessibility tree, apart from the API that serves it: the read
//! projected onto the wire's nodes and the search for one automation id, both within
//! [`MAX_NODES`] elements and [`MAX_DEPTH`] levels.
//!
//! A provider that does not answer in time fails the walk at once with a typed timeout: a hung app
//! never reads as a tree of empty defaults. An element that went away while it was read is left
//! out, and a property its provider does not offer reads as the field's default, which is the
//! provider's own answer.

use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::error::{ErrorCode, HandleKind, Retry};
use flui_protocol::tree::{ElementId, Node, Tree};

use crate::error::{Error, Result};

/// The most elements one walk visits.
pub const MAX_NODES: usize = 4096;

/// The deepest level one walk visits.
pub const MAX_DEPTH: usize = 64;

/// Why a provider could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    /// It did not answer within the client's timeout.
    Timeout,
    /// The element went away.
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
        truncated: false,
        timeout,
    };
    match walk.visit(provider, root, 0)? {
        Some(node) => Ok(Tree::new(vec![node], walk.truncated)),
        None => Err(failed("the window's element went away")),
    }
}

struct Walk {
    next_id: u64,
    truncated: bool,
    timeout: Duration,
}

impl Walk {
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
        let first = self.answer(provider.first_child(element))?;
        if depth >= MAX_DEPTH {
            if first.is_some() {
                node.children_unread = true;
                self.truncated = true;
            }
            return Ok(Some(node));
        }
        let mut child = first;
        while let Some(current) = child {
            if usize::try_from(self.next_id).unwrap_or(usize::MAX) > MAX_NODES {
                node.children_unread = true;
                self.truncated = true;
                break;
            }
            if let Some(read) = self.visit(provider, &current, depth + 1)? {
                node.children.push(read);
            }
            child = self.answer(provider.next_sibling(&current))?;
        }
        Ok(Some(node))
    }

    /// A step of the walk: a gone element ends its level, a timeout or a failure the walk.
    fn answer<E>(&self, step: Result<Option<E>, ProviderError>) -> Result<Option<E>> {
        match step {
            Ok(next) => Ok(next),
            Err(ProviderError::Gone) => Ok(None),
            Err(error) => Err(provider_error(error, self.timeout)),
        }
    }
}

/// The first element under `root`, in document order, whose automation id is `automation_id`.
///
/// # Errors
///
/// [`Error::NotFound`] when no element within the walk's budgets has it, saying whether the
/// budget cut the search short; [`Error::Timeout`] and [`Error::Platform`] as [`read`].
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
/// budget cut the search short; [`Error::Timeout`] and [`Error::Platform`] as [`read`].
pub fn find_by<P: Provider>(
    provider: &mut P,
    root: P::Element,
    mut matches: impl FnMut(&Node) -> bool,
    wanted: &str,
    timeout: Duration,
) -> Result<P::Element> {
    let walk = Walk {
        next_id: 1,
        truncated: false,
        timeout,
    };
    let mut visited = 0_usize;
    let mut cut_short = false;
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
            Err(ProviderError::Gone) => None,
            Err(error) => return Err(provider_error(error, timeout)),
        };
        if node.is_some_and(|node| matches(&node)) {
            return Ok(element);
        }
        if depth > 1 {
            *slot = walk.answer(provider.next_sibling(&element))?;
        }
        if depth > MAX_DEPTH {
            if walk.answer(provider.first_child(&element))?.is_some() {
                cut_short = true;
            }
            continue;
        }
        let child = walk.answer(provider.first_child(&element))?;
        if child.is_some() {
            levels.push(child);
        }
    }
    Err(Error::not_found(format!(
        "the OS accessibility tree shows no element {wanted}{}",
        if cut_short {
            format!(" within its first {MAX_NODES} elements and {MAX_DEPTH} levels")
        } else {
            String::new()
        }
    )))
}
