//! Accessibility semantics widgets.
//!
//! These widgets are thin `RenderView` wrappers over the semantics proxy render
//! objects in `flui-objects`, matching Flutter's `Semantics`,
//! `MergeSemantics`, and `ExcludeSemantics` split.
//!
//! # The `Send + Sync` bound on action handlers comes from storage, not from threading
//!
//! [`Semantics`]'s `on_*` builders register handlers that assistive technology
//! can invoke. The stored handler type is
//! `Arc<dyn Fn(..) + Send + Sync>` (`flui_semantics::SemanticsActionHandler`),
//! so the builders require that bound. A caller meets the consequence
//! immediately and it is worth stating here rather than leaving to a compiler
//! error: the ordinary "activation toggles this control's own state" closure
//! does not compile, because the state a widget keeps is `Rc<RefCell<_>>` and
//! that is neither `Send` nor `Sync`. Reaching for `Arc<Mutex<_>>` or a shared
//! store is the way through.
//!
//! The bound is inherited, not chosen. The handler lives inside a
//! `SemanticsConfiguration`, which rides in the semantics proxy render object,
//! whose `RenderView::RenderObject` associated type is pinned `Send + Sync`
//! (`flui_view::RenderView`). **The handler is not invoked across a thread**:
//! action resolution is owner-local and commits at the pipeline's Idle point.
//! The point at which a platform adapter really does cross threads is one layer
//! out, in `flui_platform`, at the seam where the platform's own thread hands
//! work to the owner.
//!
//! It also diverges from this catalog's dominant convention rather than being a
//! novelty in it: the widget crates carry 56 `Rc<dyn Fn(..)>` callback aliases
//! (44 in `flui-widgets/src`, 12 in `flui-material/src`), and ten existing
//! public builders already take `impl Fn(..) + Send + Sync + 'static`
//! (`interaction/draggable.rs`, `interaction/drag_target.rs`,
//! `scroll/page_view.rs`). The market survey, the rejected alternatives, and the
//! reason the reference's shape does not transcribe are recorded in
//! `ARCHITECTURE.md` §17.

use std::sync::Arc;

use flui_objects::{
    RenderExcludeSemantics, RenderIndexedSemantics, RenderMergeSemantics,
    RenderSemanticsAnnotations,
};
use flui_rendering::{
    protocol::BoxProtocol,
    semantics::{
        ActionArgs, SemanticsAction, SemanticsActionHandler, SemanticsConfiguration,
        SemanticsProperties, SemanticsRole, TextDirection,
    },
};
use flui_view::{Child, IntoView, RenderView, impl_render_view};

#[derive(Clone, Copy, Debug, Default)]
struct SemanticsOptions {
    bits: u8,
}

impl SemanticsOptions {
    const CONTAINER: u8 = 1 << 0;
    const EXPLICIT_CHILD_NODES: u8 = 1 << 1;
    const EXCLUDE_DESCENDANTS: u8 = 1 << 2;
    const BLOCK_USER_ACTIONS: u8 = 1 << 3;

    #[inline]
    const fn contains(self, flag: u8) -> bool {
        (self.bits & flag) != 0
    }

    #[inline]
    fn set(&mut self, flag: u8, value: bool) {
        if value {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
    }
}

/// Annotates a subtree with accessibility semantics.
#[derive(Clone, Debug)]
// PORT-CHECK-OK-SP3: widget view type; `flui_rendering::pipeline::Semantics` is a typestate phase marker, not the accessibility widget/config object
pub struct Semantics {
    configuration: SemanticsConfiguration,
    options: SemanticsOptions,
    child: Child,
}

impl Default for Semantics {
    fn default() -> Self {
        Self {
            configuration: SemanticsConfiguration::new(),
            options: SemanticsOptions::default(),
            child: Child::empty(),
        }
    }
}

impl Semantics {
    /// Creates an empty semantics annotation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a semantics annotation from the shared properties bag.
    pub fn from_properties(properties: &SemanticsProperties) -> Self {
        Self {
            configuration: SemanticsConfiguration::from_properties(properties),
            ..Self::default()
        }
    }

    /// Creates a semantics annotation from a ready configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            ..Self::default()
        }
    }

    /// Set whether this widget introduces a new semantics node.
    #[must_use]
    pub fn container(mut self, container: bool) -> Self {
        self.options.set(SemanticsOptions::CONTAINER, container);
        self
    }

    /// Set whether descendants must create explicit semantics nodes.
    #[must_use]
    pub fn explicit_child_nodes(mut self, explicit_child_nodes: bool) -> Self {
        self.options
            .set(SemanticsOptions::EXPLICIT_CHILD_NODES, explicit_child_nodes);
        self
    }

    /// Set whether descendant semantics are ignored.
    #[must_use]
    pub fn exclude_semantics(mut self, exclude_semantics: bool) -> Self {
        self.options
            .set(SemanticsOptions::EXCLUDE_DESCENDANTS, exclude_semantics);
        self
    }

    /// Set whether user-action semantics are blocked for descendants.
    #[must_use]
    pub fn block_user_actions(mut self, block_user_actions: bool) -> Self {
        self.options
            .set(SemanticsOptions::BLOCK_USER_ACTIONS, block_user_actions);
        self
    }

    /// Set the accessible label.
    #[must_use]
    pub fn label(mut self, label: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_label(label);
        self
    }

    /// Set the accessible value.
    #[must_use]
    pub fn value(mut self, value: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_value(value);
        self
    }

    /// Set the accessible hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_hint(hint);
        self
    }

    /// Set whether this node is enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.configuration.set_enabled(Some(enabled));
        self
    }

    /// Set whether this node has checked state and is checked.
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.configuration.set_checked(Some(checked));
        self
    }

    /// Set whether this node belongs to a mutually-exclusive group.
    ///
    /// On a checkable node this flag is what distinguishes a radio button
    /// from a checkbox: the platform translation resolves checked state
    /// inside a mutually-exclusive group to a radio-button role, and checked
    /// state without it to a checkbox role.
    #[must_use]
    pub fn in_mutually_exclusive_group(mut self, in_mutually_exclusive_group: bool) -> Self {
        self.configuration
            .set_in_mutually_exclusive_group(in_mutually_exclusive_group);
        self
    }

    /// Set whether this node is in a mixed checkbox state.
    #[must_use]
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.configuration.set_mixed(mixed);
        self
    }

    /// Set whether this node has toggled state and is toggled.
    #[must_use]
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.configuration.set_toggled(Some(toggled));
        self
    }

    /// Set whether this node is selected.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.configuration.set_selected(selected);
        self
    }

    /// Set whether this node has expanded state and is expanded.
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.configuration.set_expanded(expanded);
        self
    }

    /// Set whether this node is a button.
    #[must_use]
    pub fn button(mut self, button: bool) -> Self {
        self.configuration.set_button(button);
        self
    }

    /// Set whether this node is a link.
    #[must_use]
    pub fn link(mut self, link: bool) -> Self {
        self.configuration.set_link(link);
        self
    }

    /// Set whether this node is a slider.
    #[must_use]
    pub fn slider(mut self, slider: bool) -> Self {
        self.configuration.set_slider(slider);
        self
    }

    /// Set whether this node is a header.
    #[must_use]
    pub fn header(mut self, header: bool) -> Self {
        self.configuration.set_header(header);
        self
    }

    /// Set whether this node is an image.
    #[must_use]
    pub fn image(mut self, image: bool) -> Self {
        self.configuration.set_image(image);
        self
    }

    /// Set whether this node is a text field.
    #[must_use]
    pub fn text_field(mut self, text_field: bool) -> Self {
        self.configuration.set_text_field(text_field);
        self
    }

    /// Set whether this node is read-only.
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.configuration.set_read_only(read_only);
        self
    }

    /// Set whether this node is focusable.
    #[must_use]
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.configuration.set_focusable(focusable);
        self
    }

    /// Set whether this node is focused.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.configuration.set_focused(focused);
        self
    }

    /// Set whether this node is hidden from accessibility.
    #[must_use]
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.configuration.set_hidden(hidden);
        self
    }

    /// Set whether this node is obscured, such as a password field.
    #[must_use]
    pub fn obscured(mut self, obscured: bool) -> Self {
        self.configuration.set_obscured(obscured);
        self
    }

    /// Set whether this node is multiline.
    #[must_use]
    pub fn multiline(mut self, multiline: bool) -> Self {
        self.configuration.set_multiline(multiline);
        self
    }

    /// Set whether this node scopes a route.
    #[must_use]
    pub fn scopes_route(mut self, scopes_route: bool) -> Self {
        self.configuration.set_scopes_route(scopes_route);
        self
    }

    /// Set whether this node names a route.
    #[must_use]
    pub fn names_route(mut self, names_route: bool) -> Self {
        self.configuration.set_names_route(names_route);
        self
    }

    /// Set whether this node is a live region.
    #[must_use]
    pub fn live_region(mut self, live_region: bool) -> Self {
        self.configuration.set_live_region(live_region);
        self
    }

    /// Set the text direction used when merging text semantics.
    #[must_use]
    pub fn text_direction(mut self, text_direction: TextDirection) -> Self {
        self.configuration.set_text_direction(text_direction);
        self
    }

    /// Set the platform semantics role.
    #[must_use]
    pub fn role(mut self, role: SemanticsRole) -> Self {
        self.configuration.set_role(role);
        self
    }

    // -----------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------

    /// Invoke `handler` when assistive technology activates this node.
    ///
    /// The action a screen reader's activate gesture produces, advertised to
    /// the platform as a click — this is what makes a custom-drawn control
    /// pressable without a pointer.
    #[must_use]
    pub fn on_tap(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::Tap, handler);
        self
    }

    /// Invoke `handler` when assistive technology requests a context menu.
    #[must_use]
    pub fn on_long_press(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::LongPress, handler);
        self
    }

    /// Invoke `handler` when assistive technology scrolls this node left.
    #[must_use]
    pub fn on_scroll_left(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::ScrollLeft, handler);
        self
    }

    /// Invoke `handler` when assistive technology scrolls this node right.
    #[must_use]
    pub fn on_scroll_right(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::ScrollRight, handler);
        self
    }

    /// Invoke `handler` when assistive technology scrolls this node up.
    #[must_use]
    pub fn on_scroll_up(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::ScrollUp, handler);
        self
    }

    /// Invoke `handler` when assistive technology scrolls this node down.
    #[must_use]
    pub fn on_scroll_down(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::ScrollDown, handler);
        self
    }

    /// Invoke `handler` when assistive technology increments this node's value.
    ///
    /// The one-step-up counterpart to [`Self::on_decrease`], for sliders and
    /// steppers whose value a screen reader can adjust without a pointer.
    #[must_use]
    pub fn on_increase(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::Increase, handler);
        self
    }

    /// Invoke `handler` when assistive technology decrements this node's value.
    #[must_use]
    pub fn on_decrease(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::Decrease, handler);
        self
    }

    /// Invoke `handler` when assistive technology asks for this node to be
    /// brought into view.
    ///
    /// How a screen reader moves a scrollable to whatever it is reading
    /// *next*, which is not always something the pointer path can trigger —
    /// the platform asks for a node that is currently offscreen.
    #[must_use]
    pub fn on_show_on_screen(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::ShowOnScreen, handler);
        self
    }

    /// Invoke `handler` when assistive technology moves focus to this node.
    #[must_use]
    pub fn on_focus(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::Focus, handler);
        self
    }

    /// Invoke `handler` when assistive technology moves focus away from this node.
    ///
    /// The notification counterpart to [`Self::on_focus`], and deliberately not
    /// its mirror image: the platform reports losing focus as a notification
    /// about something that already happened, whereas a focus request is a
    /// command the node may refuse.
    #[must_use]
    pub fn on_blur(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_action_handler(SemanticsAction::DidLoseAccessibilityFocus, handler);
        self
    }

    /// Invoke `handler` with the text a platform asked this node to hold.
    ///
    /// How a screen reader enters text into a custom-drawn field. A request
    /// that arrives without a text payload is dropped with a trace rather than
    /// passed on as an empty string — `""` is a legitimate edit a platform
    /// could mean, so synthesizing one would turn a lost payload into a silent
    /// erasure of the field's contents.
    #[must_use]
    pub fn on_set_text(mut self, handler: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.configuration.add_action(
            SemanticsAction::SetText,
            Arc::new(move |_, args| {
                if let Some(ActionArgs::SetText { text }) = args {
                    handler(&text);
                } else {
                    tracing::warn!(
                        "dropping a set-text request whose payload did not cross the translation \
                         seam; the node's existing content is left unchanged"
                    );
                }
            }),
        );
        self
    }

    /// Invoke `handler` with the offset a platform asked this node to scroll to.
    ///
    /// As with [`Self::on_set_text`], a request that arrives without an offset
    /// payload is dropped with a trace rather than passed on as `(0.0, 0.0)` —
    /// the origin is a position a platform can legitimately mean, so inventing
    /// it would scroll the view somewhere the request never asked for.
    #[must_use]
    pub fn on_scroll_to_offset(
        mut self,
        handler: impl Fn(f64, f64) + Send + Sync + 'static,
    ) -> Self {
        self.configuration.add_action(
            SemanticsAction::ScrollToOffset,
            Arc::new(move |_, args| {
                if let Some(ActionArgs::ScrollToOffset { x, y }) = args {
                    handler(x, y);
                } else {
                    tracing::warn!(
                        "dropping a scroll-to-offset request whose payload did not cross the \
                         translation seam; the scroll position is left unchanged"
                    );
                }
            }),
        );
        self
    }

    /// Registers `handler` for `action` verbatim, without adapting its shape.
    ///
    /// The escape hatch for the two things the typed builders deliberately do
    /// not cover: the actions the platform vocabulary cannot route yet, and
    /// callers that need the handler's identity to survive a rebuild. The
    /// handler arrives already wrapped, as a [`SemanticsActionHandler`], and is
    /// stored as given.
    ///
    /// Identity matters because this widget's configuration is compared with
    /// `Arc::ptr_eq` — a fresh handler allocated on every `build` therefore
    /// makes the configuration compare unequal and raises a semantics dirty
    /// impact on every rebuild, even when nothing semantic changed. Building
    /// the handler once and cloning the `Arc` keeps that quiet:
    ///
    /// ```rust
    /// use std::sync::Arc;
    ///
    /// use flui_rendering::RenderUpdateImpact;
    /// use flui_rendering::semantics::{SemanticsAction, SemanticsActionHandler};
    /// use flui_view::{RenderObjectContext, RenderView as _};
    /// use flui_widgets::Semantics;
    ///
    /// // Built once, where the widget's own state lives:
    /// let activate: SemanticsActionHandler = Arc::new(|_action, _arguments| {});
    ///
    /// // Each build clones that handler into a fresh widget, so a rebuild
    /// // carries the same handler and the mounted configuration compares equal:
    /// let building = || {
    ///     Semantics::new()
    ///         .label("Play")
    ///         .on_action(SemanticsAction::Tap, Arc::clone(&activate))
    /// };
    ///
    /// let mut mounted = building().create_render_object(&RenderObjectContext::detached());
    /// assert_eq!(
    ///     building().update_render_object(&RenderObjectContext::detached(), &mut mounted),
    ///     RenderUpdateImpact::NONE,
    /// );
    /// ```
    ///
    /// A configuration built by hand and passed to [`Self::from_configuration`]
    /// reaches the same surface, but only wholesale — this builder adds one
    /// action to an annotation assembled by the other builders.
    #[must_use]
    pub fn on_action(mut self, action: SemanticsAction, handler: SemanticsActionHandler) -> Self {
        self.configuration.add_action(action, handler);
        self
    }

    /// Registers a no-argument action handler.
    ///
    /// The single adaptation point from a widget-facing closure to the stored
    /// `SemanticsActionHandler`, which takes both the action and its optional
    /// payload. Both are dropped deliberately here: a typed builder already
    /// knows which action it is bound to, so the discriminant carries nothing
    /// its caller does not have, and every action routed through this helper
    /// takes no arguments.
    fn add_action_handler(
        &mut self,
        action: SemanticsAction,
        handler: impl Fn() + Send + Sync + 'static,
    ) {
        self.configuration
            .add_action(action, Arc::new(move |_, _| handler()));
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Semantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSemanticsAnnotations;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSemanticsAnnotations::from_configuration(self.configuration.clone())
            .with_container(self.options.contains(SemanticsOptions::CONTAINER))
            .with_explicit_child_nodes(
                self.options
                    .contains(SemanticsOptions::EXPLICIT_CHILD_NODES),
            )
            .with_exclude_semantics(self.options.contains(SemanticsOptions::EXCLUDE_DESCENDANTS))
            .with_block_user_actions(self.options.contains(SemanticsOptions::BLOCK_USER_ACTIONS))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_configuration(self.configuration.clone());
        impact |= render_object.set_container(self.options.contains(SemanticsOptions::CONTAINER));
        impact |= render_object.set_explicit_child_nodes(
            self.options
                .contains(SemanticsOptions::EXPLICIT_CHILD_NODES),
        );
        impact |= render_object
            .set_exclude_semantics(self.options.contains(SemanticsOptions::EXCLUDE_DESCENDANTS));
        impact |= render_object
            .set_block_user_actions(self.options.contains(SemanticsOptions::BLOCK_USER_ACTIONS));
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Semantics);

/// Merges the semantics of all descendants into a single node.
#[derive(Clone, Debug, Default)]
pub struct MergeSemantics {
    child: Child,
}

impl MergeSemantics {
    /// Creates a merge-semantics widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for MergeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMergeSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderMergeSemantics::default()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(MergeSemantics);

/// Annotates its child's semantics node with a zero-based index among its
/// siblings.
///
/// This is the "12" a screen reader announces in "item 12 of 100"; the "100"
/// comes from the enclosing scrollable's own child count, not from here.
///
/// Flutter's `IndexedSemantics`, and in the reference every lazy sliver
/// delegate wraps every materialised item in one by default. FLUI's do not —
/// see flui-rendering's `## Mapping decisions` — so this is for content you
/// index yourself: a hand-built list, a grid of cards, anything a lazy sliver
/// does not own.
///
/// The index is zero-based on both sides; the one-based conversion AccessKit's
/// `position_in_set` wants happens once, at the platform boundary.
#[derive(Clone, Debug, Default)]
pub struct IndexedSemantics {
    index: i32,
    child: Child,
}

impl IndexedSemantics {
    /// Creates an indexed-semantics widget for a zero-based `index`.
    #[must_use]
    pub fn new(index: i32) -> Self {
        Self {
            index,
            child: Child::empty(),
        }
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for IndexedSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIndexedSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIndexedSemantics::new(self.index)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_index(self.index)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IndexedSemantics);

/// Drops descendant semantics while keeping layout, paint, and hit testing.
#[derive(Clone, Debug)]
pub struct ExcludeSemantics {
    excluding: bool,
    child: Child,
}

impl Default for ExcludeSemantics {
    fn default() -> Self {
        Self {
            excluding: true,
            child: Child::empty(),
        }
    }
}

impl ExcludeSemantics {
    /// Creates an exclude-semantics widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether descendants are excluded from semantics.
    #[must_use]
    pub fn excluding(mut self, excluding: bool) -> Self {
        self.excluding = excluding;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ExcludeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderExcludeSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderExcludeSemantics::new(self.excluding)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_excluding(self.excluding);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ExcludeSemantics);

#[cfg(test)]
mod tests {
    use flui_rendering::RenderObject;
    use flui_rendering::semantics::{AttributedString, SemanticsRole};

    use super::*;

    // ------------------------------------------------------------------
    // Semantics -- builder methods reach the built SemanticsConfiguration.
    // ------------------------------------------------------------------

    #[test]
    fn semantics_builder_methods_reach_the_configuration() {
        let widget = Semantics::new()
            .label("a label")
            .value("a value")
            .hint("a hint")
            .enabled(true)
            .checked(true)
            .mixed(true)
            .toggled(true)
            .selected(true)
            .expanded(true)
            .button(true)
            .link(true)
            .slider(true)
            .header(true)
            .image(true)
            .text_field(true)
            .read_only(true)
            .focusable(true)
            .focused(true)
            .hidden(true)
            .obscured(true)
            .multiline(true)
            .scopes_route(true)
            .names_route(true)
            .live_region(true)
            .text_direction(TextDirection::Rtl)
            .role(SemanticsRole::Dialog);

        let render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        let config = render_object.configuration();

        assert_eq!(
            config.label().map(AttributedString::as_str),
            Some("a label")
        );
        assert_eq!(
            config.value().map(AttributedString::as_str),
            Some("a value")
        );
        assert_eq!(config.hint().map(AttributedString::as_str), Some("a hint"));
        assert_eq!(config.is_enabled(), Some(true));
        assert_eq!(config.is_checked(), Some(true));
        assert!(config.is_mixed());
        assert_eq!(config.is_toggled(), Some(true));
        assert!(config.is_selected());
        assert!(config.is_expanded());
        assert!(config.is_button());
        assert!(config.is_link());
        assert!(config.is_slider());
        assert!(config.is_header());
        assert!(config.is_image());
        assert!(config.is_text_field());
        assert!(config.is_read_only());
        assert!(config.is_focusable());
        assert!(config.is_focused());
        assert!(config.is_hidden());
        assert!(config.is_obscured());
        assert!(config.is_multiline());
        assert!(config.scopes_route());
        assert!(config.names_route());
        assert!(config.is_live_region());
        assert_eq!(config.text_direction(), Some(TextDirection::Rtl));
        assert_eq!(config.role(), SemanticsRole::Dialog);
    }

    #[test]
    fn semantics_options_reach_the_render_object() {
        let widget = Semantics::new()
            .container(true)
            .explicit_child_nodes(true)
            .exclude_semantics(true)
            .block_user_actions(true);

        let render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());

        assert!(render_object.container());
        assert!(render_object.explicit_child_nodes());
        assert!(render_object.exclude_semantics());
        assert!(render_object.block_user_actions());
    }

    #[test]
    fn semantics_defaults_are_all_off() {
        let render_object =
            Semantics::new().create_render_object(&flui_view::RenderObjectContext::detached());

        assert!(!render_object.container());
        assert!(!render_object.explicit_child_nodes());
        assert!(!render_object.exclude_semantics());
        assert!(!render_object.block_user_actions());
        assert_eq!(render_object.configuration().is_enabled(), None);
    }

    #[test]
    fn semantics_update_render_object_reapplies_configuration_and_options() {
        let mut render_object =
            Semantics::new().create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(!render_object.container());

        let updated = Semantics::new()
            .label("updated")
            .container(true)
            .button(true);
        let impact = updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::SEMANTICS);

        assert!(render_object.container());
        assert!(render_object.configuration().is_button());
        assert_eq!(
            render_object
                .configuration()
                .label()
                .map(AttributedString::as_str),
            Some("updated")
        );
    }

    #[test]
    fn semantics_identical_configuration_is_none_and_all_options_union_semantics() {
        let original = Semantics::new().label("stable");
        let mut render_object =
            original.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            original.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::NONE,
        );

        let changed = original
            .clone()
            .container(true)
            .explicit_child_nodes(true)
            .exclude_semantics(true)
            .block_user_actions(true);
        assert_eq!(
            changed.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
        assert!(render_object.exclude_semantics());
        assert!(render_object.block_user_actions());
    }

    #[test]
    fn semantics_from_properties_maps_the_shared_properties_bag() {
        use flui_rendering::semantics::SemanticsProperties;

        let properties = SemanticsProperties::new()
            .with_label("from properties")
            .with_button(true);
        let render_object = Semantics::from_properties(&properties)
            .create_render_object(&flui_view::RenderObjectContext::detached());

        assert_eq!(
            render_object
                .configuration()
                .label()
                .map(AttributedString::as_str),
            Some("from properties")
        );
        assert!(render_object.configuration().is_button());
    }

    #[test]
    fn semantics_from_configuration_uses_the_given_configuration_directly() {
        let mut config = SemanticsConfiguration::new();
        config.set_selected(true);

        let render_object = Semantics::from_configuration(config)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(render_object.configuration().is_selected());
    }

    #[test]
    fn semantics_has_children_reflects_whether_a_child_was_set() {
        assert!(!Semantics::new().has_children());
        assert!(
            Semantics::new()
                .child(crate::SizedBox::new(10.0, 10.0))
                .has_children()
        );
    }

    #[test]
    fn on_action_stores_the_handler_identity_it_is_given() {
        let handler: SemanticsActionHandler = Arc::new(|_, _| {});

        let widget = Semantics::new().on_action(SemanticsAction::Tap, Arc::clone(&handler));
        let render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        let stored = render_object
            .configuration()
            .action_handler(SemanticsAction::Tap)
            .expect("on_action registered a tap handler");

        assert!(
            Arc::ptr_eq(stored, &handler),
            "the handler must be stored as given rather than re-wrapped in a \
             fresh Arc: the configuration's identity is what a rebuild is \
             compared on, so re-wrapping raises a semantics impact every frame",
        );
    }

    #[test]
    fn a_handler_allocated_per_build_costs_a_semantics_impact_per_rebuild() {
        // Two builds of one widget, each handler allocated by the typed builder
        // on the spot. Configurations compare action handlers by identity, so
        // the second build differs from the first only in that fresh `Arc` and
        // still reports a semantics impact — the cost `on_action`'s docs warn a
        // caller about, pinned here as a contract so that a change removing the
        // churn has to say so rather than quietly alter the re-publish rate of
        // every node carrying an action.
        let build = || Semantics::new().label("Play").on_tap(|| {});

        let mut render_object =
            build().create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            build().update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    // ------------------------------------------------------------------
    // MergeSemantics
    // ------------------------------------------------------------------

    #[test]
    fn merge_semantics_declares_a_boundary_that_merges_descendants() {
        let render_object =
            MergeSemantics::new().create_render_object(&flui_view::RenderObjectContext::detached());
        let mut config = SemanticsConfiguration::new();
        render_object.describe_semantics_configuration(&mut config);

        assert!(config.is_semantics_boundary());
        assert!(config.is_merging_semantics_of_descendants());
    }

    #[test]
    fn merge_semantics_has_children_reflects_whether_a_child_was_set() {
        assert!(!MergeSemantics::new().has_children());
        assert!(
            MergeSemantics::new()
                .child(crate::SizedBox::new(10.0, 10.0))
                .has_children()
        );
    }

    // ------------------------------------------------------------------
    // ExcludeSemantics
    // ------------------------------------------------------------------

    #[test]
    fn exclude_semantics_defaults_to_excluding_and_toggles_off() {
        let default_render_object = ExcludeSemantics::new()
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(default_render_object.excludes_semantics_subtree());

        let disabled = ExcludeSemantics::new()
            .excluding(false)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(!disabled.excludes_semantics_subtree());
    }

    #[test]
    fn exclude_semantics_update_render_object_reapplies_excluding() {
        let mut render_object = ExcludeSemantics::new()
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(render_object.excludes_semantics_subtree());

        let impact = ExcludeSemantics::new()
            .excluding(false)
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::SEMANTICS);
        assert!(!render_object.excludes_semantics_subtree());
    }

    #[test]
    fn exclude_semantics_has_children_reflects_whether_a_child_was_set() {
        assert!(!ExcludeSemantics::new().has_children());
        assert!(
            ExcludeSemantics::new()
                .child(crate::SizedBox::new(10.0, 10.0))
                .has_children()
        );
    }
}
