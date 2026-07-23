//! Single consolidated integration-test binary: every root tests/*.rs is included as a
//! module here (files stay in place) so cargo links ONE binary instead of 49 — cutting
//! link time and target/ disk. The shared harness is mounted once as `crate::common`.

mod common;

#[path = "absorb_pointer.rs"]
mod absorb_pointer;
#[path = "animated_size.rs"]
mod animated_size;
#[path = "baseline.rs"]
mod baseline;
#[path = "binding_animation.rs"]
mod binding_animation;
#[path = "box_extras.rs"]
mod box_extras;
#[path = "clip.rs"]
mod clip;
#[path = "component_child_ordering.rs"]
mod component_child_ordering;
#[path = "composition.rs"]
mod composition;
#[path = "container.rs"]
mod container;
#[path = "custom_multi_child_layout.rs"]
mod custom_multi_child_layout;
#[path = "custom_paint.rs"]
mod custom_paint;
#[path = "custom_single_child_layout.rs"]
mod custom_single_child_layout;
#[path = "decorated_box.rs"]
mod decorated_box;
#[path = "fade_transition.rs"]
mod fade_transition;
#[path = "fitted_box.rs"]
mod fitted_box;
#[path = "flex.rs"]
mod flex;
#[path = "flex_parent_data.rs"]
mod flex_parent_data;
#[path = "flow.rs"]
mod flow;
#[path = "gesture_detector.rs"]
mod gesture_detector;
#[path = "gesture_detector_advanced.rs"]
mod gesture_detector_advanced;
#[path = "image.rs"]
mod image;
#[path = "implicit_animations.rs"]
mod implicit_animations;
#[path = "indexed_stack.rs"]
mod indexed_stack;
#[path = "inherited_app.rs"]
mod inherited_app;
#[path = "intrinsic_and_overflow.rs"]
mod intrinsic_and_overflow;
#[path = "layout.rs"]
mod layout;
#[path = "lazy_grid.rs"]
mod lazy_grid;
#[path = "lazy_list.rs"]
mod lazy_list;
#[path = "list_body.rs"]
mod list_body;
#[path = "listener.rs"]
mod listener;
#[path = "modifiers.rs"]
mod modifiers;
#[path = "mouse_region.rs"]
mod mouse_region;
#[path = "overflow_box.rs"]
mod overflow_box;
#[path = "rich_text.rs"]
mod rich_text;
#[path = "rotation_transition.rs"]
mod rotation_transition;
#[path = "scale_transition.rs"]
mod scale_transition;
#[path = "scroll.rs"]
mod scroll;
#[path = "semantics.rs"]
mod semantics;
#[path = "shrink_wrapping_viewport.rs"]
mod shrink_wrapping_viewport;
#[path = "sliver_opacity.rs"]
mod sliver_opacity;
#[path = "spacer.rs"]
mod spacer;
#[path = "stack_positioned.rs"]
mod stack_positioned;
#[path = "stateful.rs"]
mod stateful;
#[path = "table.rs"]
mod table;
#[path = "text.rs"]
mod text;
#[path = "text_field.rs"]
mod text_field;
#[path = "visibility.rs"]
mod visibility;
#[path = "wrap.rs"]
mod wrap;
