### Removed

- `flui-view`: `WidgetsBindingObserver`, `WidgetsBinding::{add_observer, remove_observer, observer_count, handle_metrics_changed, handle_view_focus_changed, handle_pop_route, handle_push_route, handle_start_back_gesture, handle_update_back_gesture_progress, handle_commit_back_gesture, handle_cancel_back_gesture}` and the types only they used: `RouteInformation`, `PredictiveBackEvent`, `ViewFocusEvent`, `ViewFocusState` and `ViewFocusDirection`. Lifecycle observation is `LifecycleHandle::subscribe`, which returns a `LifecycleSubscription`.
