//! Semantics proxy render objects.
//!
//! Semantics annotations, merging and exclusion. Layout, paint, and
//! hit-testing are transparent single-child proxy behavior; only the
//! semantics hooks differ.

use flui_foundation::Single;
use flui_foundation::geometry::Axis;
use flui_rendering::semantics::{NumericRange, SemanticsAction};
use flui_rendering::{
    pipeline::RenderInvalidationHandle,
    view::{ScrollPosition, ViewportOffset},
};
use std::rc::Rc;

struct ScrollSubscription {
    position: ScrollPosition,
    listener: std::rc::Rc<dyn Fn()>,
}
impl std::fmt::Debug for ScrollSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollSubscription")
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}
impl Drop for ScrollSubscription {
    fn drop(&mut self) {
        self.position.remove_listener(&self.listener);
    }
}

use flui_rendering::{
    hit_testing::LocalPayloadTarget,
    parent_data::BoxParentData,
    semantics::{SemanticsActionHandler, SemanticsConfiguration, SemanticsProperties},
    traits::RenderBox,
};

/// Where a semantics node's owner-local action handlers live.
///
/// A widget whose action handlers are owner-local closures keeps them in the
/// owner's interaction lane under `target`, and advertises each action in the
/// configuration through one `Send + Sync` `handler` that resolves `target`
/// when the action is invoked. The render object stores the pair so a rebuild
/// can replace the lane payload under the same ticket and reuse the same
/// `handler`, which keeps the configuration comparing equal.
#[derive(Clone)]
pub struct SemanticsActionRoute {
    target: LocalPayloadTarget,
    handler: SemanticsActionHandler,
}

impl SemanticsActionRoute {
    /// Pair a lane ticket with the handler that resolves it.
    #[must_use]
    pub fn new(target: LocalPayloadTarget, handler: SemanticsActionHandler) -> Self {
        Self { target, handler }
    }

    /// The owner-lane ticket of the action table.
    #[must_use]
    pub fn target(&self) -> LocalPayloadTarget {
        self.target
    }

    /// The handler every routed action is advertised with.
    #[must_use]
    pub fn handler(&self) -> &SemanticsActionHandler {
        &self.handler
    }
}

impl std::fmt::Debug for SemanticsActionRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsActionRoute")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

/// A render object that annotates its subtree with semantics properties.
#[derive(Debug, Clone)]
pub struct RenderSemanticsAnnotations {
    configuration: SemanticsConfiguration,
    container: bool,
    explicit_child_nodes: bool,
    exclude_semantics: bool,
    block_user_actions: bool,
    has_child: bool,
    action_route: Option<SemanticsActionRoute>,
    scroll: Option<(ScrollPosition, Axis, bool)>,
    scroll_subscription: Option<Rc<ScrollSubscription>>,
    invalidation: Option<RenderInvalidationHandle>,
}

impl RenderSemanticsAnnotations {
    /// Creates a semantics-annotations render object from semantic properties.
    pub fn new(properties: SemanticsProperties) -> Self {
        Self::from_configuration(SemanticsConfiguration::from_properties(&properties))
    }

    /// Creates a semantics-annotations render object from a ready
    /// configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            container: false,
            explicit_child_nodes: false,
            exclude_semantics: false,
            block_user_actions: false,
            has_child: false,
            action_route: None,
            scroll: None,
            scroll_subscription: None,
            invalidation: None,
        }
    }

    /// Publishes the live viewport range and only the directions that can move.
    /// Position notifications invalidate semantics without rebuilding the viewport.
    pub fn set_scroll_source(
        &mut self,
        source: Option<(ScrollPosition, Axis, bool)>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self
            .scroll
            .as_ref()
            .zip(source.as_ref())
            .is_some_and(|(old, new)| old.0.ptr_eq(&new.0) && old.1 == new.1 && old.2 == new.2)
            || self.scroll.is_none() && source.is_none()
        {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.scroll_subscription = None;
        self.scroll = source;
        self.subscribe_scroll();
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    fn subscribe_scroll(&mut self) {
        if let (Some((position, _, _)), Some(handle)) = (&self.scroll, &self.invalidation) {
            let handle = handle.clone();
            let listener: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
                let _ = handle.mark_needs_semantics();
            });
            position.add_listener(std::rc::Rc::clone(&listener));
            self.scroll_subscription = Some(Rc::new(ScrollSubscription {
                position: position.clone(),
                listener,
            }));
        }
    }

    /// The owner-lane route of this node's action handlers, if its widget
    /// registered any. Data only: layout, paint, hit-testing and semantics
    /// assembly never read it.
    #[must_use]
    pub fn action_route(&self) -> Option<&SemanticsActionRoute> {
        self.action_route.as_ref()
    }

    /// Replace the owner-lane route, returning the previous one.
    pub fn set_action_route(
        &mut self,
        route: Option<SemanticsActionRoute>,
    ) -> Option<SemanticsActionRoute> {
        std::mem::replace(&mut self.action_route, route)
    }

    /// Returns the semantic properties configuration.
    pub fn configuration(&self) -> &SemanticsConfiguration {
        &self.configuration
    }

    /// Replaces the semantic properties configuration and reports semantics when changed.
    pub fn set_configuration(
        &mut self,
        configuration: SemanticsConfiguration,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.configuration == configuration {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.configuration = configuration;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Returns whether this object introduces a semantics boundary.
    #[inline]
    pub fn container(&self) -> bool {
        self.container
    }

    /// Sets whether this object introduces a semantics boundary.
    pub fn set_container(&mut self, container: bool) -> flui_rendering::RenderUpdateImpact {
        if self.container == container {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.container = container;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Chainable form of [`Self::set_container`].
    #[must_use]
    pub fn with_container(mut self, container: bool) -> Self {
        self.container = container;
        self
    }

    /// Returns whether descendants must create explicit semantics nodes.
    #[inline]
    pub fn explicit_child_nodes(&self) -> bool {
        self.explicit_child_nodes
    }

    /// Sets whether descendants must create explicit semantics nodes.
    pub fn set_explicit_child_nodes(
        &mut self,
        explicit_child_nodes: bool,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.explicit_child_nodes == explicit_child_nodes {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.explicit_child_nodes = explicit_child_nodes;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Chainable form of [`Self::set_explicit_child_nodes`].
    #[must_use]
    pub fn with_explicit_child_nodes(mut self, explicit_child_nodes: bool) -> Self {
        self.explicit_child_nodes = explicit_child_nodes;
        self
    }

    /// Returns whether descendant semantics are ignored.
    #[inline]
    pub fn exclude_semantics(&self) -> bool {
        self.exclude_semantics
    }

    /// Sets whether descendant semantics are ignored.
    pub fn set_exclude_semantics(
        &mut self,
        exclude_semantics: bool,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.exclude_semantics == exclude_semantics {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.exclude_semantics = exclude_semantics;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Chainable form of [`Self::set_exclude_semantics`].
    #[must_use]
    pub fn with_exclude_semantics(mut self, exclude_semantics: bool) -> Self {
        self.exclude_semantics = exclude_semantics;
        self
    }

    /// Returns whether user-action semantics are blocked for descendants.
    #[inline]
    pub fn block_user_actions(&self) -> bool {
        self.block_user_actions
    }

    /// Sets whether user-action semantics are blocked for descendants.
    pub fn set_block_user_actions(
        &mut self,
        block_user_actions: bool,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.block_user_actions == block_user_actions {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.block_user_actions = block_user_actions;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Chainable form of [`Self::set_block_user_actions`].
    #[must_use]
    pub fn with_block_user_actions(mut self, block_user_actions: bool) -> Self {
        self.block_user_actions = block_user_actions;
        self
    }
}

impl Default for RenderSemanticsAnnotations {
    fn default() -> Self {
        Self::new(SemanticsProperties::new())
    }
}

impl flui_foundation::Diagnosticable for RenderSemanticsAnnotations {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("container", self.container, "container");
        builder.add_flag(
            "explicit_child_nodes",
            self.explicit_child_nodes,
            "explicit child nodes",
        );
        builder.add_flag(
            "exclude_semantics",
            self.exclude_semantics,
            "exclude semantics",
        );
        builder.add_flag(
            "block_user_actions",
            self.block_user_actions,
            "block user actions",
        );
        builder.add_flag(
            "has_semantics",
            self.configuration.has_been_annotated(),
            "has semantics",
        );
        // The two route flags a route's page wrapper sets, so a widget test can
        // assert the wrapper scopes (and names) a route.
        builder.add_flag(
            "scopes_route",
            self.configuration.scopes_route(),
            "scopes route",
        );
        builder.add_flag(
            "names_route",
            self.configuration.names_route(),
            "names route",
        );
        // The shape of a `GestureDetector`'s action-advertising helper node,
        // so a test asking for a control's own wrapper can leave it out.
        builder.add_flag(
            "actions_only",
            self.configuration.is_actions_only(),
            "actions only",
        );
        // The shape of a `Focus` widget's focus-state node, left out the same
        // way.
        builder.add_flag(
            "focus_state_only",
            self.configuration.is_focus_state_only(),
            "focus state only",
        );
    }
}

impl RenderBox for RenderSemanticsAnnotations {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        *config = self.configuration.clone();
        config.set_semantics_boundary(self.container);
        config.set_explicit_child_nodes(self.explicit_child_nodes);
        config.set_blocks_user_actions(self.block_user_actions);
        if let Some((position, axis, reversed)) = &self.scroll {
            let min = position.min_scroll_extent();
            let max = position.max_scroll_extent();
            let pixels = position.pixels();
            config.set_scroll_position(pixels);
            config.set_scroll_axis(*axis);
            config.set_scroll_extent_min(min);
            config.set_scroll_extent_max(max);
            if config.has_action(SemanticsAction::SetNumericValue)
                && let Ok(range) = NumericRange::new(
                    pixels.clamp(min, max),
                    min,
                    max,
                    (position.viewport_dimension() * 0.8).max(f64::MIN_POSITIVE),
                )
            {
                config.set_numeric_range(range);
            }
            let (decrease, increase) = match axis {
                Axis::Vertical => (SemanticsAction::ScrollUp, SemanticsAction::ScrollDown),
                Axis::Horizontal => (SemanticsAction::ScrollLeft, SemanticsAction::ScrollRight),
            };
            if pixels <= min {
                config.remove_action(SemanticsAction::Decrease);
                config.remove_action(if *reversed { increase } else { decrease });
            }
            if pixels >= max {
                config.remove_action(SemanticsAction::Increase);
                config.remove_action(if *reversed { decrease } else { increase });
            }
        }
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.scroll_subscription = None;
        self.invalidation = Some(handle);
        self.subscribe_scroll();
    }
    fn detach(&mut self) {
        self.invalidation = None;
        self.scroll_subscription = None;
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.exclude_semantics
    }
}

/// A render object that annotates its child's semantics node with an index
/// among its siblings.
///
/// The index is the "12" a screen reader announces in "item 12 of 100", and is
/// **zero-based**; the one-based conversion AccessKit's
/// `position_in_set` wants happens once, at the platform boundary.
#[derive(Debug, Clone)]
pub struct RenderIndexedSemantics {
    index: i32,
    has_child: bool,
}

impl RenderIndexedSemantics {
    /// Creates an indexed-semantics render object.
    #[must_use]
    pub const fn new(index: i32) -> Self {
        Self {
            index,
            has_child: false,
        }
    }

    /// The zero-based index this node reports.
    #[must_use]
    pub const fn index(&self) -> i32 {
        self.index
    }

    /// Sets the index, reporting whether semantics must be republished.
    ///
    /// An unchanged index reports [`RenderUpdateImpact::NONE`](flui_rendering::RenderUpdateImpact::NONE). Index churn is the norm on
    /// a scrolling list (every item's wrapper is rebuilt as the band moves),
    /// so a setter that always marked would republish the whole subtree's
    /// semantics on every frame of a scroll.
    pub fn set_index(&mut self, index: i32) -> flui_rendering::RenderUpdateImpact {
        if self.index == index {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.index = index;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl flui_foundation::Diagnosticable for RenderIndexedSemantics {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("index", i64::from(self.index));
    }
}

impl RenderBox for RenderIndexedSemantics {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        config.set_index_in_parent(self.index);
    }
}

/// A render object that merges all descendant semantics into one node.
#[derive(Debug, Clone, Default)]
pub struct RenderMergeSemantics {
    has_child: bool,
}

impl RenderBox for RenderMergeSemantics {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        config.set_semantics_boundary(true);
        config.set_merging_semantics_of_descendants(true);
    }
}

impl flui_foundation::Diagnosticable for RenderMergeSemantics {}

/// A render object that drops its descendant semantics while leaving layout,
/// paint, and hit testing unchanged.
#[derive(Debug, Clone)]
pub struct RenderExcludeSemantics {
    excluding: bool,
    has_child: bool,
}

impl RenderExcludeSemantics {
    /// Creates an exclude-semantics render object.
    pub const fn new(excluding: bool) -> Self {
        Self {
            excluding,
            has_child: false,
        }
    }

    /// Returns whether descendant semantics are excluded.
    #[inline]
    pub fn excluding(&self) -> bool {
        self.excluding
    }

    /// Sets whether descendant semantics are excluded.
    pub fn set_excluding(&mut self, excluding: bool) -> flui_rendering::RenderUpdateImpact {
        if self.excluding == excluding {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.excluding = excluding;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl Default for RenderExcludeSemantics {
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderExcludeSemantics {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("excluding", self.excluding, "excluding");
    }
}

impl RenderBox for RenderExcludeSemantics {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    fn excludes_semantics_subtree(&self) -> bool {
        self.excluding
    }
}
