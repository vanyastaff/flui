//! Application Type and Builder
//!
//! This module provides the core `App` type that represents a FLUI application
//! and integrates properly with the existing FLUI ecosystem.

use flui_view::StatelessView;
use std::marker::PhantomData;

/// Core application type wrapping a root view
///
/// This is the main application container that holds the root view
/// and provides metadata about the application.
///
/// # Type Parameters
///
/// - `V`: The root view type implementing `StatelessView`
///
/// # Thread Safety
///
/// `App<V>` is `Send + Sync` if `V` is `Send + Sync`.
#[derive(Debug, Clone)]
pub struct App<V>
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    /// Root view of the application
    pub view: V,

    /// Application metadata
    pub metadata: AppMetadata,
}

impl<V> App<V>
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    /// Create a new application with the given root view
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let app = App::new(MyRootView { title: "Hello".to_string() });
    /// ```
    pub fn new(view: V) -> Self {
        Self {
            view,
            metadata: AppMetadata::default(),
        }
    }

    /// Create an application with custom metadata
    pub fn with_metadata(view: V, metadata: AppMetadata) -> Self {
        Self { view, metadata }
    }

    /// Get the root view
    pub fn root_view(&self) -> &V {
        &self.view
    }

    /// Update the root view (useful for hot reload)
    pub fn update_view(mut self, view: V) -> Self {
        self.view = view;
        self
    }

    /// Get application metadata
    pub fn metadata(&self) -> &AppMetadata {
        &self.metadata
    }

    /// Update application metadata
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.metadata.name = Some(name.into());
        self
    }

    /// Set application version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.metadata.version = Some(version.into());
        self
    }
}

/// Application metadata and identification
#[derive(Debug, Clone, Default)]
pub struct AppMetadata {
    /// Application name/title
    pub name: Option<String>,

    /// Application version
    pub version: Option<String>,

    /// Application identifier (bundle ID, package name, etc.)
    pub identifier: Option<String>,

    /// Application description
    pub description: Option<String>,
}

/// Application builder for fluent configuration
///
/// Provides a convenient builder pattern for creating applications
/// with custom metadata and configuration.
pub struct AppBuilder<V>
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    view: Option<V>,
    metadata: AppMetadata,
}

impl<V> AppBuilder<V>
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    /// Create a new application builder
    pub fn new() -> Self {
        Self {
            view: None,
            metadata: AppMetadata::default(),
        }
    }

    /// Set the root view
    pub fn view(mut self, view: V) -> Self {
        self.view = Some(view);
        self
    }

    /// Set application name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.metadata.name = Some(name.into());
        self
    }

    /// Set application version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.metadata.version = Some(version.into());
        self
    }

    /// Set application identifier
    pub fn identifier(mut self, id: impl Into<String>) -> Self {
        self.metadata.identifier = Some(id.into());
        self
    }

    /// Set application description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.metadata.description = Some(desc.into());
        self
    }

    /// Build the final application
    ///
    /// # Panics
    ///
    /// Panics if no view was set.
    pub fn build(self) -> App<V> {
        let view = self.view.expect("View must be set before building app");
        App {
            view,
            metadata: self.metadata,
        }
    }

    /// Build and run the application immediately
    ///
    /// Convenience method that builds the app and runs it with default configuration.
    ///
    /// # Panics
    ///
    /// Panics if no view was set.
    pub fn run(self) -> ! {
        let app = self.build();
        crate::run_app_with_config(app, crate::config::AppConfig::default())
    }
}

impl<V> Default for AppBuilder<V>
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_element::Element;
    use flui_view::{BuildContext, IntoElement, StatelessView};

    #[derive(Debug, Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            Element::placeholder()
        }
    }

    #[test]
    fn test_app_creation() {
        let app = App::new(TestView);
        assert!(app.metadata.name.is_none());
        assert!(app.metadata.version.is_none());
    }

    #[test]
    fn test_app_with_metadata() {
        let app = App::new(TestView)
            .with_name("Test App")
            .with_version("1.0.0");

        assert_eq!(app.metadata.name, Some("Test App".to_string()));
        assert_eq!(app.metadata.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_app_builder() {
        let app = AppBuilder::new()
            .view(TestView)
            .name("Builder Test")
            .version("2.0.0")
            .description("Built with builder pattern")
            .build();

        assert_eq!(app.metadata.name, Some("Builder Test".to_string()));
        assert_eq!(app.metadata.version, Some("2.0.0".to_string()));
        assert_eq!(
            app.metadata.description,
            Some("Built with builder pattern".to_string())
        );
    }

    #[test]
    #[should_panic(expected = "View must be set before building app")]
    fn test_builder_without_view_panics() {
        let _app = AppBuilder::<TestView>::new().name("No View App").build();
    }
}
