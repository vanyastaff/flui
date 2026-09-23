//! The untyped field-recording method takes a token only `flui-view` can
//! construct, so an empty or foreign `FieldSet` cannot be registered.
use std::any::TypeId;

use flui_view::{BuildContext, FieldSet};

fn sneak(ctx: &dyn BuildContext) {
    let token = flui_view::context::build_context::sealed::CrateToken::new();
    ctx.depend_on_inherited_fields(token, TypeId::of::<u8>(), FieldSet::NONE, &mut |_| {});
}

fn main() {}
