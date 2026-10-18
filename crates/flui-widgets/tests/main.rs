//! Single consolidated integration-test binary: every root tests/*.rs is included as a
//! module here (files stay in place) so cargo links ONE binary instead of 49 — cutting
//! link time and target/ disk. The shared harness is mounted once as `crate::common`.

#![allow(
    clippy::unwrap_used,
    reason = "scenario functions are rows of the contract tables, so clippy no longer sees them as test functions"
)]

mod catalog_disclosure;
mod catalog_slider;
mod collapsing_header;
mod common;
mod context_menu_area;
mod contracts;
mod frame_partitions;
mod gesture_alternatives;
mod gesture_settings;
mod interactive_viewer;
mod keyboard_focus;
mod keyboard_modifiers;
mod nested_scroll;
mod pointer_vocabulary;
mod refresh_indicator;

#[path = "absorb_pointer.rs"]
mod absorb_pointer;
#[path = "actions.rs"]
mod actions;
#[path = "activity_indicator.rs"]
mod activity_indicator;
mod anchored_drag;
#[path = "animated_list.rs"]
mod animated_list;
#[path = "animated_size.rs"]
mod animated_size;
#[path = "back_gesture.rs"]
mod back_gesture;
#[path = "binding_animation.rs"]
mod binding_animation;
#[path = "child_type_swap.rs"]
mod child_type_swap;
#[path = "component_child_ordering.rs"]
mod component_child_ordering;
#[path = "composition.rs"]
mod composition;
#[path = "controller_labels.rs"]
mod controller_labels;
#[path = "custom_multi_child_layout.rs"]
mod custom_multi_child_layout;
#[path = "directionality_dependency.rs"]
mod directionality_dependency;
#[path = "dismissible.rs"]
mod dismissible;
#[path = "drag_and_drop.rs"]
mod drag_and_drop;
#[path = "editable_text.rs"]
mod editable_text;
#[path = "editable_text_clipboard.rs"]
mod editable_text_clipboard;
#[path = "flex_parent_data.rs"]
mod flex_parent_data;
#[path = "focus.rs"]
mod focus;
#[path = "form.rs"]
mod form;
#[path = "future_builder.rs"]
mod future_builder;
#[path = "gesture_detector.rs"]
mod gesture_detector;
#[path = "gesture_detector_advanced.rs"]
mod gesture_detector_advanced;
mod haptics;
#[path = "hero.rs"]
mod hero;
#[path = "hero_controller.rs"]
mod hero_controller;
#[path = "hero_flight.rs"]
mod hero_flight;
#[path = "hero_gesture.rs"]
mod hero_gesture;
#[path = "hero_public.rs"]
mod hero_public;
#[path = "hero_seam.rs"]
mod hero_seam;
mod hero_waypoint_tracks;
#[path = "hit_slop.rs"]
mod hit_slop;
#[path = "hot_reload_state.rs"]
mod hot_reload_state;
#[path = "implicit_animations.rs"]
mod implicit_animations;
#[path = "layer_inspection.rs"]
mod layer_inspection;
#[path = "layout_builder.rs"]
mod layout_builder;
#[path = "lazy_grid.rs"]
mod lazy_grid;
#[path = "lazy_list.rs"]
mod lazy_list;
#[path = "listenable_relays.rs"]
mod listenable_relays;
#[path = "listener.rs"]
mod listener;
#[path = "localizations.rs"]
mod localizations;
#[path = "media_query_fields.rs"]
mod media_query_fields;
#[path = "modal_route.rs"]
mod modal_route;
#[path = "motion.rs"]
mod motion;
#[path = "navigator.rs"]
mod navigator;
#[path = "navigator_public.rs"]
mod navigator_public;
/// Issue #536: an `Opacity` rebuild reaches the composited layer.
#[path = "overlay.rs"]
mod overlay;
#[path = "owner_code.rs"]
mod owner_code;
#[path = "page_route.rs"]
mod page_route;
#[path = "page_transitions.rs"]
mod page_transitions;
#[path = "page_view.rs"]
mod page_view;
#[path = "page_view_events.rs"]
mod page_view_events;
#[path = "parent_data_ancestry.rs"]
mod parent_data_ancestry;
#[path = "raw_button.rs"]
mod raw_button;
#[path = "routable_derive.rs"]
mod routable_derive;
#[path = "router.rs"]
mod router;
#[path = "scroll.rs"]
mod scroll;
#[path = "semantics.rs"]
mod semantics;
#[path = "shortcuts.rs"]
mod shortcuts;
#[path = "signals.rs"]
mod signals;
#[path = "signals_legal_shapes.rs"]
mod signals_legal_shapes;
#[path = "sliver_persistent_header.rs"]
mod sliver_persistent_header;
#[path = "stack_positioned.rs"]
mod stack_positioned;
#[path = "stream_builder.rs"]
mod stream_builder;
mod swipe_actions;
#[path = "system_gesture_exclusion.rs"]
mod system_gesture_exclusion;
#[path = "table.rs"]
mod table;
#[path = "text.rs"]
mod text;
#[path = "text_field.rs"]
mod text_field;
#[path = "text_field_widget.rs"]
mod text_field_widget;
#[path = "text_store_kit.rs"]
mod text_store_kit;
#[path = "transition_route.rs"]
mod transition_route;
#[path = "transitions.rs"]
mod transitions;
#[path = "visibility.rs"]
mod visibility;
#[path = "widgets_app.rs"]
mod widgets_app;
#[path = "widgets_app_router.rs"]
mod widgets_app_router;
#[path = "wrap.rs"]
mod wrap;
