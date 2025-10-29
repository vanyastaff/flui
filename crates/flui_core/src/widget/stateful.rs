//! StatefulWidget - widgets with mutable state
//!
//! StatefulWidget creates a State object that persists across rebuilds.
//! The widget itself is immutable configuration, but the State can mutate.
//!
//! # When to Use
//!
//! - Widget needs mutable state that persists
//! - Widget responds to user interactions
//! - Widget needs lifecycle hooks
//! - Widget performs animations or async operations
//!
//! # Architecture
//!
//! ```text
//! StatefulWidget (immutable config)
//!   ↓ creates
//! State (mutable, persists)
//!   ↓ builds
//! Widget tree
//! ```
//!
//! # Examples
//!
//! ```
//! use flui_core::{StatefulWidget, State, BoxedWidget};
//!
//! #[derive(Debug)]
//! struct Counter {
//!     initial: i32,
//! }
//!
//! struct CounterState {
//!     count: i32,
//! }
//!
//! impl StatefulWidget for Counter {
//!     type State = CounterState;
//!
//!     fn create_state(&self) -> Self::State {
//!         CounterState { count: self.initial }
//!     }
//! }
//!
//! impl State<Counter> for CounterState {
//!     fn build(&mut self, widget: &Counter) -> BoxedWidget {
//!         Box::new(Text::new(format!("Count: {}", self.count)))
//!     }
//! }
//!
//! // Widget and DynWidget are automatic!
//! ```

use crate::BoxedWidget;
use std::fmt;

/// StatefulWidget - widget that creates a State object
///
/// This is the trait for widgets that need mutable state.
/// The widget itself is immutable configuration, but it creates
/// a State object that can mutate.
///
/// # Separation of Concerns
///
/// - **Widget**: Immutable configuration (recreated on rebuild)
/// - **State**: Mutable state (persists across rebuilds)
///
/// ```text
/// Frame 1: Counter{initial:0} → CounterState{count:0}
///                                       ↓ user clicks
/// Frame 2: Counter{initial:0} → CounterState{count:1} (same state!)
/// ```
///
/// # Lifecycle
///
/// ```text
/// 1. Widget created: Counter { initial: 0 }
/// 2. create_state() → CounterState { count: 0 }
/// 3. State.init() called
/// 4. State.build() → widget tree
/// 5. User interaction → State.increment()
/// 6. mark_dirty() → State.build() again
/// 7. Widget updated: Counter { initial: 10 }
/// 8. State.did_update_widget() called
/// 9. State.build() again
/// ...
/// N. Element disposed → State.dispose()
/// ```
///
/// # Performance
///
/// - **State persists** - Not recreated on rebuild
/// - **Widget cheap** - Typically small structs
/// - **Rebuild fast** - Only State.build() called
///
/// # Examples
///
/// ## Simple Counter
///
/// ```
/// #[derive(Debug)]
/// struct Counter {
///     initial: i32,
///     step: i32,
/// }
///
/// struct CounterState {
///     count: i32,
///     step: i32,
/// }
///
/// impl StatefulWidget for Counter {
///     type State = CounterState;
///
///     fn create_state(&self) -> Self::State {
///         CounterState {
///             count: self.initial,
///             step: self.step,
///         }
///     }
/// }
///
/// impl State<Counter> for CounterState {
///     fn build(&mut self, widget: &Counter) -> BoxedWidget {
///         Box::new(Column::new(vec![
///             Box::new(Text::new(format!("Count: {}", self.count))),
///             Box::new(Button::new("Increment", || {
///                 self.count += self.step;
///             })),
///         ]))
///     }
///
///     fn did_update_widget(&mut self, old: &Counter, new: &Counter) {
///         if old.step != new.step {
///             self.step = new.step;
///         }
///     }
/// }
/// ```
///
/// ## Form with Validation
///
/// ```
/// #[derive(Debug)]
/// struct LoginForm {
///     initial_username: String,
/// }
///
/// struct LoginFormState {
///     username: String,
///     password: String,
///     error: Option<String>,
/// }
///
/// impl StatefulWidget for LoginForm {
///     type State = LoginFormState;
///
///     fn create_state(&self) -> Self::State {
///         LoginFormState {
///             username: self.initial_username.clone(),
///             password: String::new(),
///             error: None,
///         }
///     }
/// }
///
/// impl State<LoginForm> for LoginFormState {
///     fn build(&mut self, widget: &LoginForm) -> BoxedWidget {
///         Box::new(Column::new(vec![
///             Box::new(TextField::new("Username")
///                 .value(&self.username)
///                 .on_change(|text| self.username = text)),
///
///             Box::new(TextField::new("Password")
///                 .value(&self.password)
///                 .obscure_text(true)
///                 .on_change(|text| self.password = text)),
///
///             if let Some(error) = &self.error {
///                 Box::new(Text::new(error).color(Color::RED))
///             } else {
///                 Box::new(SizedBox::empty())
///             },
///
///             Box::new(Button::new("Login", || self.submit())),
///         ]))
///     }
/// }
///
/// impl LoginFormState {
///     fn submit(&mut self) {
///         if self.username.is_empty() {
///             self.error = Some("Username required".into());
///         } else if self.password.len() < 8 {
///             self.error = Some("Password too short".into());
///         } else {
///             // Perform login
///             self.error = None;
///         }
///     }
/// }
/// ```
///
/// ## Async Data Loading
///
/// ```
/// use std::sync::Arc;
/// use tokio::sync::RwLock;
///
/// #[derive(Debug)]
/// struct UserProfile {
///     user_id: u64,
/// }
///
/// enum LoadingState<T> {
///     Loading,
///     Loaded(T),
///     Error(String),
/// }
///
/// struct UserProfileState {
///     data: Arc<RwLock<LoadingState<User>>>,
/// }
///
/// impl StatefulWidget for UserProfile {
///     type State = UserProfileState;
///
///     fn create_state(&self) -> Self::State {
///         UserProfileState {
///             data: Arc::new(RwLock::new(LoadingState::Loading)),
///         }
///     }
/// }
///
/// impl State<UserProfile> for UserProfileState {
///     fn init(&mut self, widget: &UserProfile) {
///         // Start async load
///         let data = self.data.clone();
///         let user_id = widget.user_id;
///
///         tokio::spawn(async move {
///             match fetch_user(user_id).await {
///                 Ok(user) => {
///                     *data.write().await = LoadingState::Loaded(user);
///                 }
///                 Err(e) => {
///                     *data.write().await = LoadingState::Error(e.to_string());
///                 }
///             }
///         });
///     }
///
///     fn build(&mut self, widget: &UserProfile) -> BoxedWidget {
///         match &*self.data.blocking_read() {
///             LoadingState::Loading => {
///                 Box::new(LoadingSpinner::new())
///             }
///             LoadingState::Loaded(user) => {
///                 Box::new(UserCard::new(user))
///             }
///             LoadingState::Error(err) => {
///                 Box::new(ErrorWidget::new(err))
///             }
///         }
///     }
/// }
/// ```
pub trait StatefulWidget: Clone + fmt::Debug + Send + Sync + 'static {
    /// The type of state this widget creates
    ///
    /// This is the mutable state object that persists across rebuilds.
    type State: State<Self> + Send + Sync;

    /// Create the initial state
    ///
    /// Called once when the element is first created.
    /// The returned state persists for the lifetime of the element.
    ///
    /// # Examples
    ///
    /// ```
    /// impl StatefulWidget for Counter {
    ///     type State = CounterState;
    ///
    ///     fn create_state(&self) -> Self::State {
    ///         CounterState {
    ///             count: self.initial,
    ///         }
    ///     }
    /// }
    /// ```
    fn create_state(&self) -> Self::State;

    /// Create boxed state wrapped for DynState
    ///
    /// This is a helper that creates the state and wraps it in StateWrapper
    /// so it can be used as Box<dyn DynState>.
    ///
    /// You don't need to override this - the default implementation is correct.
    fn create_boxed_state(&self) -> crate::element::stateful::BoxedState
    where
        Self: Sized + 'static,
        Self::State: fmt::Debug,
    {
        Box::new(StateWrapper::<Self>::new(self.create_state()))
    }
}

/// State - mutable state for StatefulWidget
///
/// This is the mutable counterpart to StatefulWidget.
/// It persists across rebuilds and can be mutated in response
/// to user interactions or async events.
///
/// # Lifecycle Hooks
///
/// - `init()` - Called once after state is created
/// - `build()` - Called to build widget tree (every rebuild)
/// - `did_update_widget()` - Called when widget config changes
/// - `dispose()` - Called before state is destroyed
///
/// # State Mutation
///
/// You can mutate state in:
/// - Event handlers (closures)
/// - Async callbacks
/// - Lifecycle hooks
///
/// After mutation, call `mark_dirty()` to schedule rebuild.
///
/// # Examples
///
/// ## Basic State
///
/// ```
/// struct CounterState {
///     count: i32,
/// }
///
/// impl State<Counter> for CounterState {
///     fn build(&mut self, widget: &Counter) -> BoxedWidget {
///         Box::new(Text::new(format!("{}", self.count)))
///     }
/// }
/// ```
///
/// ## State with Lifecycle
///
/// ```
/// struct TimerState {
///     elapsed: Duration,
///     handle: Option<JoinHandle<()>>,
/// }
///
/// impl State<Timer> for TimerState {
///     fn init(&mut self, widget: &Timer) {
///         // Start timer
///         let handle = tokio::spawn(async {
///             // Timer logic
///         });
///         self.handle = Some(handle);
///     }
///
///     fn build(&mut self, widget: &Timer) -> BoxedWidget {
///         Box::new(Text::new(format!("{:?}", self.elapsed)))
///     }
///
///     fn dispose(&mut self) {
///         // Clean up
///         if let Some(handle) = self.handle.take() {
///             handle.abort();
///         }
///     }
/// }
/// ```
pub trait State<W: StatefulWidget>: Send + Sync + 'static {
    /// Initialize state
    ///
    /// Called once after state is created. Use this for:
    /// - Starting async operations
    /// - Subscribing to streams
    /// - Setting up listeners
    ///
    /// # Examples
    ///
    /// ```
    /// fn init(&mut self, widget: &MyWidget) {
    ///     println!("State initialized");
    ///
    ///     // Start async operation
    ///     self.load_data(widget.user_id);
    /// }
    /// ```
    fn init(&mut self, _widget: &W) {
        // Default: do nothing
    }

    /// Build the widget tree
    ///
    /// Called whenever the widget needs to rebuild.
    /// This is where you construct the UI based on current state.
    ///
    /// # Parameters
    ///
    /// - `widget` - Current widget configuration
    /// - `context` - BuildContext for accessing inherited widgets
    ///
    /// # Returns
    ///
    /// Widget tree representing the UI
    ///
    /// # Examples
    ///
    /// ```
    /// fn build(&mut self, widget: &Counter, context: &BuildContext) -> BoxedWidget {
    ///     // Access theme via BuildContext
    ///     let theme = context.depend_on::<Theme>().unwrap();
    ///
    ///     Box::new(Column::new(vec![
    ///         Box::new(Text::new(format!("Count: {}", self.count))),
    ///         Box::new(Button::new("Increment", || self.increment())),
    ///     ]))
    /// }
    /// ```
    fn build(&mut self, widget: &W, context: &crate::element::BuildContext) -> BoxedWidget;

    /// Called when widget configuration changes
    ///
    /// Use this to update state when widget config changes.
    ///
    /// # Parameters
    ///
    /// - `old_widget` - Previous widget configuration
    /// - `new_widget` - New widget configuration
    ///
    /// # Examples
    ///
    /// ```
    /// fn did_update_widget(&mut self, old: &Counter, new: &Counter) {
    ///     if old.step != new.step {
    ///         println!("Step changed: {} -> {}", old.step, new.step);
    ///         self.step = new.step;
    ///     }
    /// }
    /// ```
    fn did_update_widget(&mut self, _old_widget: &W, _new_widget: &W) {
        // Default: do nothing
    }

    /// Called before state is destroyed
    ///
    /// Use this to clean up resources:
    /// - Cancel async operations
    /// - Unsubscribe from streams
    /// - Remove listeners
    ///
    /// # Examples
    ///
    /// ```
    /// fn dispose(&mut self) {
    ///     println!("State disposed");
    ///
    ///     // Cancel async operation
    ///     if let Some(handle) = self.task_handle.take() {
    ///         handle.abort();
    ///     }
    /// }
    /// ```
    fn dispose(&mut self) {
        // Default: do nothing
    }
}

/// Automatic Widget implementation for StatefulWidget
///
/// All StatefulWidget types automatically get Widget trait,
/// which in turn automatically get DynWidget via blanket impl.
///
/// # Element Type
///
/// StatefulWidget uses `StatefulElement<Self>` which:
/// - Creates and stores the State object
/// - Calls State.build() to get widget tree
/// - Manages state lifecycle (init, update, dispose)
///
/// # State Type
///
/// Uses the associated `State` type from StatefulWidget
///
/// # Arity
///
// Widget impl is now generated by #[derive(StatefulWidget)] macro
// This avoids blanket impl conflicts on stable Rust
//
// Use: #[derive(StatefulWidget)] on your widget type

// DynWidget comes automatically via blanket impl in mod.rs!

// Wrapper to make State → DynState conversion explicit
// This holds the concrete State<W> and knows the widget type W
struct StateWrapper<W: StatefulWidget> {
    state: W::State,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: StatefulWidget> StateWrapper<W> {
    fn new(state: W::State) -> Self {
        Self {
            state,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<W> crate::element::stateful::DynState for StateWrapper<W>
where
    W: StatefulWidget + 'static,
    W::State: fmt::Debug,
{
    fn build(
        &mut self,
        widget: &dyn crate::DynWidget,
        context: &crate::element::BuildContext,
    ) -> crate::BoxedWidget {
        // Downcast widget to concrete type W
        if let Some(concrete_widget) = (widget as &dyn std::any::Any).downcast_ref::<W>() {
            // Call the concrete State::build()
            State::build(&mut self.state, concrete_widget, context)
        } else {
            // This shouldn't happen - widget type mismatch
            panic!("Widget type mismatch in DynState::build()");
        }
    }

    fn did_update_widget(
        &mut self,
        old_widget: &dyn crate::DynWidget,
        new_widget: &dyn crate::DynWidget,
    ) {
        if let (Some(old), Some(new)) = (
            (old_widget as &dyn std::any::Any).downcast_ref::<W>(),
            (new_widget as &dyn std::any::Any).downcast_ref::<W>(),
        ) {
            State::did_update_widget(&mut self.state, old, new);
        }
    }

    fn dispose(&mut self) {
        State::dispose(&mut self.state);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        &self.state
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        &mut self.state
    }
}

impl<W> fmt::Debug for StateWrapper<W>
where
    W: StatefulWidget,
    W::State: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateWrapper")
            .field("state", &self.state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    #[test]
    fn test_simple_stateful_widget() {
        #[derive(Debug, Clone)]
        struct TestWidget {
            initial: i32,
        }

        struct TestState {
            value: i32,
        }

        impl StatefulWidget for TestWidget {
            type State = TestState;

            fn create_state(&self) -> Self::State {
                TestState {
                    value: self.initial,
                }
            }
        }

        impl State<TestWidget> for TestState {
            fn build(&mut self, widget: &TestWidget, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }
        }

        let widget = TestWidget { initial: 42 };

        // Create state
        let state = widget.create_state();
        assert_eq!(state.value, 42);

        // Widget is automatic
        let _: &dyn Widget = &widget;

        // DynWidget is automatic
        let _: &dyn crate::DynWidget = &widget;
    }

    #[test]
    fn test_state_lifecycle() {
        #[derive(Debug, Clone)]
        struct LifecycleWidget;

        struct LifecycleState {
            init_called: bool,
            update_called: bool,
            dispose_called: bool,
        }

        impl StatefulWidget for LifecycleWidget {
            type State = LifecycleState;

            fn create_state(&self) -> Self::State {
                LifecycleState {
                    init_called: false,
                    update_called: false,
                    dispose_called: false,
                }
            }
        }

        impl State<LifecycleWidget> for LifecycleState {
            fn init(&mut self, _widget: &LifecycleWidget) {
                self.init_called = true;
            }

            fn build(&mut self, _widget: &LifecycleWidget, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }

            fn did_update_widget(&mut self, _old: &LifecycleWidget, _new: &LifecycleWidget) {
                self.update_called = true;
            }

            fn dispose(&mut self) {
                self.dispose_called = true;
            }
        }

        let widget = LifecycleWidget;
        let mut state = widget.create_state();

        // Test init
        assert!(!state.init_called);
        state.init(&widget);
        assert!(state.init_called);

        // Test update
        assert!(!state.update_called);
        state.did_update_widget(&widget, &widget);
        assert!(state.update_called);

        // Test dispose
        assert!(!state.dispose_called);
        state.dispose();
        assert!(state.dispose_called);
    }

    #[test]
    fn test_stateful_widget_without_clone() {
        // StatefulWidget requires Clone for Widget trait
        #[derive(Debug, Clone)]
        struct NonCloneWidget {
            data: Vec<u8>,
        }

        struct NonCloneState;

        impl StatefulWidget for NonCloneWidget {
            type State = NonCloneState;

            fn create_state(&self) -> Self::State {
                NonCloneState
            }
        }

        impl State<NonCloneWidget> for NonCloneState {
            fn build(&mut self, _widget: &NonCloneWidget, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }
        }

        let widget = NonCloneWidget {
            data: vec![1, 2, 3],
        };

        // Can still box it
        let boxed: crate::BoxedWidget = Box::new(widget);
        assert!(boxed.is::<NonCloneWidget>());
    }

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockWidget;

    impl Widget for MockWidget {
        // Element type determined by framework
    }

    impl crate::DynWidget for MockWidget {
        fn clone_boxed(&self) -> crate::BoxedWidget {
            Box::new(self.clone())
        }
    }

    #[derive(Debug)]
    struct MockElement;

    impl<W: Widget> crate::Element<W> for MockElement {
        fn new(_: W) -> Self {
            Self
        }
    }
}
