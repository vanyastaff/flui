use flui::view::WriterSource;

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn owner_writer_source(source: WriterSource) {
    require_thread_spawnable(source);
}

fn main() {
    let _ = owner_writer_source;
}
