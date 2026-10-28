use flui::animation::SharedCurve;
use flui::cupertino::CupertinoTabController;
use flui::rendering::HitMetadata;
use flui::widgets::{WidgetStateProperty, WidgetStatesController};

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn owner_values(
    tabs: CupertinoTabController,
    states: WidgetStatesController,
    property: WidgetStateProperty<u32>,
    curve: SharedCurve,
    metadata: HitMetadata,
) {
    require_thread_spawnable(tabs);
    require_thread_spawnable(states);
    require_thread_spawnable(property);
    require_thread_spawnable(curve);
    require_thread_spawnable(metadata);
}

fn main() {
    let _ = owner_values;
}
