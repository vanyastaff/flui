use std::sync::Arc;

use flui_widgets::prelude::*;
use flui_widgets::{NavigatorHandle, NavigatorObserver, Router, SizedBox, WidgetsApp};

#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
}

#[derive(Debug)]
struct Analytics;

impl NavigatorObserver for Analytics {}

fn router() -> Router<AppRoute> {
    Router::new(AppRoute::Home, |_route: &AppRoute, _cx| {
        SizedBox::shrink().into_view().boxed()
    })
}

fn main() {
    // The Router owns its navigator: the router form has neither builder.
    let _ = WidgetsApp::router(router()).navigator(NavigatorHandle::new());
    let _ = WidgetsApp::router(router()).observer(Arc::new(Analytics));
}
