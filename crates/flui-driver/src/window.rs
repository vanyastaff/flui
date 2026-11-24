//! The handles a test writes against: a [`Window`], an [`ElementLocator`] (a description,
//! re-resolved at every use) and an [`Element`] (one resolved element, its handle
//! generation-checked by the app). Each borrows the [`Driver`] and mirrors the method table: a
//! gesture method on a locator or an element is the table's `element.<gesture>`, with the options
//! the builder set.

use std::time::{Duration, Instant};

use flui_protocol::act::{ActOutcome, Actionability, ActionabilityParams};
use flui_protocol::geometry::LogicalPoint;
use flui_protocol::input::{
    ActionSequence, Direction, DragParams, GestureOptions, InputParams, InputPath, InputPolicy,
    Journal, JournalParams, LongPressParams, PinchParams, PressKeysParams, Released,
    ScrollIntoViewParams, SwipeParams, TapParams, TypeTextParams,
};
use flui_protocol::locator::{LocateParams, Locator, Matches, Target};
use flui_protocol::method::{self, Method as _};
use flui_protocol::session::{Placement, WindowParams};
use flui_protocol::snapshot::Snapshot;
use flui_protocol::tree::{ElementRef, ReadParams, ReadQuery, Tree, WindowId};
use flui_protocol::wait::{Condition, WaitOutcome};

use crate::compose;
use crate::driver::Driver;
use crate::error::Result;
use crate::lane::AccessibilityApi;
use crate::projection::Projection;
use crate::scenario::WindowSpec;

/// How long [`Window::os_tree`] and [`Window::compare_projection`] read the OS tree against the
/// app's again while the app's tree moves between the two reads, before they report it busy,
/// counted from the end of the first reading: a bound in time, since an app that republishes
/// every frame moves on any count of readings, and counted after one, since one reading through
/// a device's lane (a `uiautomator dump`) can take longer than the bound.
const TREE_SETTLE: Duration = Duration::from_secs(2);

/// The fewest readings made before a moving tree is reported busy, however long they took.
const MIN_TREE_READINGS: usize = 3;

/// What reading the OS accessibility tree answers in a session without an OS lane.
fn no_tree_lane() -> crate::Error {
    crate::Error::not_supported("os_accessibility_tree", "the session has no OS lane")
}

/// One window of the app.
pub struct Window<'d> {
    pub(crate) driver: &'d mut Driver,
    pub(crate) id: WindowId,
}

impl<'d> Window<'d> {
    pub(crate) fn new(driver: &'d mut Driver, id: WindowId) -> Self {
        Self { driver, id }
    }

    /// The window's handle.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// The elements `locator` describes in this window, resolved at each use.
    pub fn locate(&mut self, locator: Locator) -> ElementLocator<'_> {
        ElementLocator::new(self.driver, locator.in_window(self.id))
    }

    /// The element `element`, read before.
    pub fn element(&mut self, element: ElementRef) -> Element<'_> {
        Element::new(self.driver, element)
    }

    /// The window's semantics tree.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn read(&mut self) -> Result<Tree> {
        self.read_with(ReadQuery::new())
    }

    /// The part of the tree `query` asks for, within its budgets.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn read_with(&mut self, query: ReadQuery) -> Result<Tree> {
        self.driver
            .agent::<method::SemanticsRead>(ReadParams::new(self.id).query(query), Duration::ZERO)
    }

    /// The window's size, scale, position and focus.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn placement(&mut self) -> Result<Placement> {
        self.driver
            .agent::<method::WindowPlacement>(WindowParams::new(self.id), Duration::ZERO)
    }

    /// The window as the OS accessibility API shows it, through the session's lane, projected
    /// onto the wire's nodes ([`Lane::read_tree`](crate::Lane::read_tree)), handed the app's own
    /// tree of the window, read first, when the lane matches against it. Its nodes' ids number
    /// them in the order they were read: they are no handles the app issued.
    ///
    /// # Errors
    ///
    /// As [`Self::read`]; [`Error::NotSupported`](crate::Error::NotSupported) naming
    /// `os_accessibility_tree` without a lane that reads one; the lane's failures;
    /// [`Error::Busy`](crate::Error::Busy) when the app's tree changed while the OS tree was read
    /// against it on each of at least three readings, through two seconds after the first.
    pub fn os_tree(&mut self) -> Result<Tree> {
        if self.lane_matches_app_tree()? {
            return self.read_against().map(|(_, os)| os);
        }
        self.os_tree_against(Tree::new(Vec::new(), false))
    }

    /// The app's tree of the window and its OS tree read against it, both of one revision: the
    /// app's revision is read again once the OS tree is, and a tree that moved in between is
    /// read again, at least [`MIN_TREE_READINGS`] times and until [`TREE_SETTLE`] has passed
    /// since the first reading ended, so a change of the app is never taken for a
    /// difference of the OS. A match the lane could not make (`NotFound`, `Ambiguous`) while the
    /// tree moved is read again the same way: it was made against a tree the app no longer
    /// showed. A read that reports no revision is taken as it is.
    ///
    /// A tree that never holds still for one reading (a clock, a progress bar republishing every
    /// frame) is reported busy rather than compared: the OS tree was taken at an unknown moment
    /// between two app trees, and matching or comparing it against either would credit the OS
    /// with the app's own change, or hide a difference behind it.
    ///
    /// # Errors
    ///
    /// As [`Self::read`] and [`Self::os_tree_against`]; [`Error::Busy`](crate::Error::Busy)
    /// naming the revisions when the tree moved on every one of those readings.
    fn read_against(&mut self) -> Result<(Tree, Tree)> {
        let mut settle_from: Option<Instant> = None;
        let mut readings = 0_usize;
        loop {
            let app = self.read()?;
            let os = self.os_tree_against(app.clone());
            readings += 1;
            let Some(before) = app.revision else {
                return os.map(|os| (app, os));
            };
            if let Err(error) = &os
                && !matches!(
                    error,
                    crate::Error::NotFound { .. } | crate::Error::Ambiguous { .. }
                )
            {
                return os.map(|os| (app, os));
            }
            let after = self.read_with(ReadQuery::new().max_depth(0))?.revision;
            if after == Some(before) {
                return os.map(|os| (app, os));
            }
            // The bound starts once the first reading is over.
            let settling = *settle_from.get_or_insert_with(Instant::now);
            if readings >= MIN_TREE_READINGS && settling.elapsed() >= TREE_SETTLE {
                let moved = after.map_or_else(
                    || format!("revision {before} became one that reports none"),
                    |after| format!("revision {before} became {after}"),
                );
                return Err(crate::Error::busy(format!(
                    "the app's tree changed while the OS tree was read against it, on each of \
                     {readings} readings over more than {} ms after the first (last, {moved}): \
                     the OS tree is not compared with a tree the app no longer shows",
                    TREE_SETTLE.as_millis()
                )));
            }
        }
    }

    /// Whether the session's lane finds the app's nodes by matching the app's own tree, which it
    /// is then handed ([`AccessibilityApi::matches_app_tree`]).
    fn lane_matches_app_tree(&mut self) -> Result<bool> {
        let lane = self.driver.lane().ok_or_else(no_tree_lane)?;
        Ok(lane
            .capabilities()
            .accessibility
            .is_some_and(AccessibilityApi::matches_app_tree))
    }

    /// [`Self::os_tree`], the lane handed `app` ([`Lane::read_tree`](crate::Lane::read_tree)).
    fn os_tree_against(&mut self, app: Tree) -> Result<Tree> {
        let placement = self.placement()?;
        let lane = self.driver.lane().ok_or_else(no_tree_lane)?;
        lane.run(move |lane| lane.read_tree(&placement, &app))?
    }

    /// Activate the element whose identifier is `identifier` through the OS accessibility API, as
    /// a screen reader does ([`Lane::invoke`](crate::Lane::invoke)): proof that assistive
    /// technology can act on it, not only read it. The lane is handed the app's own tree of the
    /// window, read first, when it matches against it and its API acts at all
    /// ([`AccessibilityApi::activates`]): a lane that refuses is not preceded by a read whose
    /// failure would stand in for its refusal.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`](crate::Error::NotSupported) naming `os_invoke` without a lane that
    /// acts through one; the lane's and the app's failures.
    pub fn os_invoke(&mut self, identifier: &str) -> Result<()> {
        let no_lane = || crate::Error::not_supported("os_invoke", "the session has no OS lane");
        let matches = self
            .driver
            .lane()
            .ok_or_else(no_lane)?
            .capabilities()
            .accessibility
            .is_some_and(|api| api.matches_app_tree() && api.activates());
        let app = if matches {
            self.read()?
        } else {
            Tree::new(Vec::new(), false)
        };
        let placement = self.placement()?;
        let lane = self.driver.lane().ok_or_else(no_lane)?;
        let identifier = identifier.to_owned();
        lane.run(move |lane| lane.invoke(&placement, &identifier, &app))?
    }

    /// The window's semantics tree and its OS accessibility tree, each as its normalized outline
    /// (ADR-0095 §4): whether assistive technology reads what the app published.
    ///
    /// # Errors
    ///
    /// As [`Self::read`] and [`Self::os_tree`]: the two trees are of one revision of the app's.
    pub fn compare_projection(&mut self) -> Result<Projection> {
        let (app, os) = self.read_against()?;
        Ok(Projection::of(&app, &os))
    }

    /// Whether the window is the one `spec` describes.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`](crate::Error::InvalidArgument) naming the expected and the
    /// actual size and scale when it is not.
    pub fn check_size(&mut self, spec: WindowSpec) -> Result<()> {
        spec.check(&self.placement()?)
    }

    /// Hold out for `condition` on the window's frame and event timeline, at most `timeout` of
    /// the app's clock; a wait that runs out is an answer, `unsatisfied`, with why the window
    /// had not settled.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn wait_until(&mut self, condition: Condition, timeout: Duration) -> Result<WaitOutcome> {
        compose::wait_until(self.driver, self.id, condition, timeout)
    }

    /// Perform `actions` by the session's input path.
    ///
    /// # Errors
    ///
    /// As `input.perform`.
    pub fn perform(&mut self, actions: ActionSequence) -> Result<ActOutcome> {
        let params = InputParams::new(self.id, actions);
        self.driver
            .command(method::InputPerform::NAME, params, compose::perform)
    }

    /// Perform `actions` by `policy`'s input path.
    ///
    /// # Errors
    ///
    /// As `input.perform`.
    pub fn perform_via(
        &mut self,
        actions: ActionSequence,
        policy: InputPolicy,
    ) -> Result<ActOutcome> {
        let mut params = InputParams::new(self.id, actions);
        params.via = Some(policy);
        self.driver
            .command(method::InputPerform::NAME, params, compose::perform)
    }

    /// Tap at `at`, in the window's logical surface coordinates: the explicit last resort.
    ///
    /// # Errors
    ///
    /// As `element.tap`.
    pub fn tap_at(&mut self, at: LogicalPoint) -> Result<ActOutcome> {
        let params = TapParams::new(Target::Point {
            window: self.id,
            at,
        });
        self.driver
            .command(method::ElementTap::NAME, params, compose::tap)
    }

    /// Release every contact and key the agent holds for this window.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn release_input(&mut self) -> Result<Released> {
        self.driver
            .agent::<method::InputRelease>(WindowParams::new(self.id), Duration::ZERO)
    }

    /// The input the window's journal still holds.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn journal(&mut self) -> Result<Journal> {
        self.driver
            .agent::<method::InputJournal>(JournalParams::new(self.id), Duration::ZERO)
    }
}

impl std::fmt::Debug for Window<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// The gesture methods both handles share, each the table's `element.<gesture>` with the handle's
/// target and options.
macro_rules! gestures {
    () => {
        /// Prefer `path` for this handle's actions, falling back when the target lacks it.
        #[must_use]
        pub fn via(mut self, path: InputPath) -> Self {
            self.options.via = Some(InputPolicy::prefer(path));
            self
        }

        /// Take `path` only, refusing any other.
        #[must_use]
        pub fn require(mut self, path: InputPath) -> Self {
            self.options.via = Some(InputPolicy::require(path));
            self
        }

        /// Skip every actionability check but `attached`.
        #[must_use]
        pub fn force(mut self) -> Self {
            self.options.force = true;
            self
        }

        /// Give finding the element actionable `timeout`; the session's when not set.
        #[must_use]
        pub fn timeout(mut self, timeout: Duration) -> Self {
            self.options.timeout_ms = Some(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX));
            self
        }

        /// Read `snapshot` back after each action.
        #[must_use]
        pub fn snapshot(mut self, snapshot: Snapshot) -> Self {
            self.options.snapshot = snapshot;
            self
        }

        /// Tap it.
        ///
        /// # Errors
        ///
        /// As `element.tap`: not found, ambiguous, not actionable, a path the policy requires
        /// that the target lacks, the app's own failure.
        pub fn tap(&mut self) -> Result<ActOutcome> {
            let mut params = TapParams::new(self.target());
            params.options = self.options;
            self.driver.command(method::ElementTap::NAME, params, compose::tap)
        }

        /// Tap it twice, as one double tap.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn double_tap(&mut self) -> Result<ActOutcome> {
            let mut params = TapParams::new(self.target());
            params.options = self.options;
            self.driver.command(method::ElementDoubleTap::NAME, params, compose::double_tap)
        }

        /// Press and hold it, `hold` long or the long-press delay and a margin.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn long_press(&mut self, hold: Option<Duration>) -> Result<ActOutcome> {
            let params = serde_json::json!({
                "target": self.target(),
                "duration_ms": hold.map(|hold| u64::try_from(hold.as_millis()).unwrap_or(u64::MAX)),
            });
            let params: LongPressParams = with_options(params, self.options)?;
            self.driver.command(method::ElementLongPress::NAME, params, compose::long_press)
        }

        /// Press it, move to `to`, release.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`], for either end.
        pub fn drag_to(&mut self, to: impl Into<Target>) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target(), "to": to.into() });
            let params: DragParams = with_options(params, self.options)?;
            self.driver.command(method::ElementDragTo::NAME, params, compose::drag_to)
        }

        /// A stroke from it towards `direction` that ends below the fling velocity.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn swipe(&mut self, direction: Direction) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target(), "direction": direction });
            let params: SwipeParams = with_options(params, self.options)?;
            self.driver.command(method::ElementSwipe::NAME, params, |driver, params| compose::swipe(driver, params, false))
        }

        /// A stroke from it towards `direction` that ends above the fling velocity.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn fling(&mut self, direction: Direction) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target(), "direction": direction });
            let params: SwipeParams = with_options(params, self.options)?;
            self.driver.command(method::ElementFling::NAME, params, |driver, params| compose::swipe(driver, params, true))
        }

        /// Two contacts around its centre, spreading (`scale` above 1) or closing.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn pinch(&mut self, scale: f64) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target(), "scale": scale });
            let params: PinchParams = with_options(params, self.options)?;
            self.driver.command(method::ElementPinch::NAME, params, compose::pinch)
        }

        /// Scroll each scrollable ancestor until it is visible.
        ///
        /// # Errors
        ///
        /// As `element.scroll_into_view`.
        pub fn scroll_into_view(&mut self) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target() });
            let params: ScrollIntoViewParams = with_options(params, self.options)?;
            self.driver.command(method::ElementScrollIntoView::NAME, params, compose::scroll_into_view)
        }

        /// Focus it, then commit `text` into it.
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn type_text(&mut self, text: &str) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target(), "text": text });
            let params: TypeTextParams = with_options(params, self.options)?;
            self.driver.command(method::ElementTypeText::NAME, params, compose::type_text)
        }

        /// Focus it, then press each chord in turn (`"Control+a"`, `"Enter"`).
        ///
        /// # Errors
        ///
        /// As [`Self::tap`].
        pub fn press_keys(&mut self, chords: &[&str]) -> Result<ActOutcome> {
            let params = serde_json::json!({ "target": self.target(), "keys": chords });
            let params: PressKeysParams = with_options(params, self.options)?;
            self.driver.command(method::ElementPressKeys::NAME, params, compose::press_keys)
        }
    };
}

/// `params` with `options` flattened in, read as a method's parameters.
fn with_options<P: serde::de::DeserializeOwned>(
    mut params: serde_json::Value,
    options: GestureOptions,
) -> Result<P> {
    let invalid = |error: serde_json::Error| {
        crate::Error::invalid_argument(format!("a gesture's parameters do not read: {error}"))
    };
    if let (Some(fields), serde_json::Value::Object(extra)) = (
        params.as_object_mut(),
        serde_json::to_value(options).map_err(invalid)?,
    ) {
        fields.extend(extra);
    }
    serde_json::from_value(params).map_err(invalid)
}

/// The elements a locator describes, resolved at every use: never a cached node.
pub struct ElementLocator<'a> {
    driver: &'a mut Driver,
    locator: Locator,
    options: GestureOptions,
}

impl<'a> ElementLocator<'a> {
    pub(crate) fn new(driver: &'a mut Driver, locator: Locator) -> Self {
        Self {
            driver,
            locator,
            options: GestureOptions::default(),
        }
    }

    fn target(&self) -> Target {
        Target::Locator(self.locator.clone())
    }

    /// The locator.
    #[must_use]
    pub fn locator(&self) -> &Locator {
        &self.locator
    }

    pub(crate) fn driver(&mut self) -> &mut Driver {
        self.driver
    }

    /// Every element it matches now.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn matches(&mut self) -> Result<Matches> {
        self.driver.agent::<method::SemanticsLocate>(
            LocateParams::new(self.locator.clone()),
            Duration::ZERO,
        )
    }

    /// The one element it matches now: strictly, unless it picks one.
    ///
    /// # Errors
    ///
    /// [`Error::Ambiguous`](crate::Error::Ambiguous) and
    /// [`Error::NotFound`](crate::Error::NotFound) after waiting for one to appear.
    pub fn resolve(self) -> Result<Element<'a>> {
        let timeout = compose::timeout_of(self.driver, &self.options);
        let handle = compose::resolve(self.driver, &self.target(), timeout)?;
        Ok(Element {
            driver: self.driver,
            handle,
            options: self.options,
        })
    }

    gestures!();
}

impl std::fmt::Debug for ElementLocator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementLocator")
            .field("locator", &self.locator)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// One element, resolved: its handle names it for as long as it stays, and answers `gone` once
/// it is removed, never another element in its slot, as the app's generational ids guarantee.
pub struct Element<'a> {
    driver: &'a mut Driver,
    handle: ElementRef,
    options: GestureOptions,
}

impl<'a> Element<'a> {
    pub(crate) fn new(driver: &'a mut Driver, handle: ElementRef) -> Self {
        Self {
            driver,
            handle,
            options: GestureOptions::default(),
        }
    }

    fn target(&self) -> Target {
        Target::Element(self.handle)
    }

    /// The element's handle, `w3/e12`.
    #[must_use]
    pub fn reference(&self) -> ElementRef {
        self.handle
    }

    /// Attached, visible, stable, enabled and receives, from one frame.
    ///
    /// # Errors
    ///
    /// When the app cannot answer.
    pub fn actionability(&mut self) -> Result<Actionability> {
        self.driver.agent::<method::ElementActionability>(
            ActionabilityParams::new(self.handle),
            Duration::ZERO,
        )
    }

    gestures!();
}

impl std::fmt::Debug for Element<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Element")
            .field("handle", &self.handle)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}
