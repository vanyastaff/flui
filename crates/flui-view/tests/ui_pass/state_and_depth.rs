use flui_view::{ElementBase, ElementDepth, LifecycleContext, WriterSource};

fn restamp(element: &mut dyn ElementBase, depth: ElementDepth) {
    element.set_depth(depth);
}


fn writer(ctx: &dyn LifecycleContext) {
    let _source: WriterSource = ctx.writer_source();
}

fn main() {
    let _ = restamp;
    let _ = writer;
}
