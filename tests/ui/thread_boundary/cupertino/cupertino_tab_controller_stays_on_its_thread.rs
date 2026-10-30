use flui::cupertino::CupertinoTabController;

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn owner_controller(controller: CupertinoTabController) {
    require_thread_spawnable(controller);
}

fn main() {
    let _ = owner_controller;
}
