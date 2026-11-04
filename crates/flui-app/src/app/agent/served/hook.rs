//! The driver agent as a development hook: bound at `attach`, handed each
//! window, stopped at `detach`.

use std::fmt;
use std::sync::Arc;

use flui_protocol::AppInfo;
use flui_view::dev_agent::{AgentWindow, DevAgentHook};

use super::super::Disconnect;
use super::launch::Launch;
use super::transport::{self, Running};

/// The driver agent: a [`DevAgentHook`] that serves the windows the host
/// hands it over the endpoint its [`Launch`] names. See the
/// [module docs](self).
pub(crate) struct DriverAgent {
    launch: Launch,
    app: AppInfo,
    /// `Some` when the launch asked to exit when the driver goes.
    disconnect: Option<Arc<Disconnect>>,
    /// Whether the loop journals its backend: the hello lists the `native` layer then.
    records_native: bool,
    /// Between a successful bind at `attach` and `detach`.
    running: Option<Running>,
}

impl DriverAgent {
    /// An agent for `launch`, introducing itself as `app`, recording the `native` layer when
    /// `records_native`. `disconnect` is
    /// what a driver's disconnect does when the launch asked to exit on one.
    pub(crate) fn new(
        launch: Launch,
        app: AppInfo,
        disconnect: Arc<Disconnect>,
        records_native: bool,
    ) -> Self {
        let disconnect = launch.exit_on_disconnect().then_some(disconnect);
        Self {
            launch,
            app,
            disconnect,
            records_native,
            running: None,
        }
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
    }
}

impl DevAgentHook for DriverAgent {
    fn attach(&mut self) -> bool {
        self.stop();
        match transport::listen(
            &self.launch,
            self.app.clone(),
            self.disconnect.clone(),
            self.records_native,
        ) {
            Ok(running) => {
                tracing::info!("driver agent listening");
                self.running = Some(running);
                true
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "the driver agent could not bind its endpoint; the app runs without it"
                );
                false
            }
        }
    }

    fn detach(&mut self) {
        self.stop();
    }

    fn window_opened(&mut self, window: AgentWindow) {
        if let Some(running) = &self.running {
            running.window_opened(window);
        }
    }
}

impl Drop for DriverAgent {
    fn drop(&mut self) {
        self.stop();
    }
}

impl fmt::Debug for DriverAgent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DriverAgent")
            .field("launch", &self.launch)
            .field("listening", &self.running.is_some())
            .finish_non_exhaustive()
    }
}
