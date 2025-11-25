//! View layer - re-exports from flui-view and flui_rendering

// === From flui-view ===
pub use flui_view::{
    // Protocol
    Animated,
    AnimatedView,
    AnimatedViewWrapper,
    // Children
    Child,
    Children,
    // Element
    Element,
    ElementTree,
    EmptyView,
    IntoElement,
    Provider,
    ProviderView,
    ProviderViewWrapper,
    Proxy,
    ProxyView,
    ProxyViewWrapper,
    Stateful,
    StatefulView,
    StatefulViewWrapper,
    Stateless,
    // Traits
    StatelessView,
    // Wrappers
    StatelessViewWrapper,
    ViewMode,
    // Types
    ViewObject,
    ViewProtocol,
    ViewState,
};

// === From flui_rendering ===
pub use flui_rendering::{RenderView, RenderViewExt, RenderViewWrapper, UpdateResult};

// === From flui-pipeline (or keep local?) ===
pub use flui_pipeline::BuildContext;

pub use root_view::{RootView, RootViewError};
