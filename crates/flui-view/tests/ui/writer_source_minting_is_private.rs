use flui_view::{BuildContext, WriterSource};

fn build_body(ctx: &dyn BuildContext) {
    let _ = WriterSource::new(ctx.reactive());
}

fn main() {}
