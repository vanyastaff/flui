//! Performance profiling with puffin and tracy

/// Profile entire function (auto-uses function name as scope)
#[macro_export]
macro_rules! profile_function {
    () => {
        #[cfg(feature = "profiling")]
        let _puffin_guard = puffin::profile_function!();

        #[cfg(feature = "tracy")]
        let _tracy_guard = tracy_client::span!();
    };
}

/// Profile a scope with custom name
#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        #[cfg(feature = "profiling")]
        let _puffin_guard = puffin::profile_scope!($name);

        #[cfg(feature = "tracy")]
        let _tracy_guard = tracy_client::span!($name);
    };
}

/// Profile an expression (returns result)
#[macro_export]
macro_rules! profile_expr {
    ($name:expr, $expr:expr) => {{
        profile_scope!($name);
        $expr
    }};
}

/// Initialize profiling (call once at startup)
#[cfg(any(feature = "profiling", feature = "tracy"))]
pub fn init() {
    #[cfg(feature = "profiling")]
    {
        puffin::set_scopes_on(true);
        tracing::info!("Puffin profiling enabled");
    }

    #[cfg(feature = "tracy")]
    {
        tracy_client::Client::start();
        tracing::info!("Tracy profiling enabled");
    }
}

/// Initialize profiling (no-op when profiling disabled)
#[cfg(not(any(feature = "profiling", feature = "tracy")))]
pub fn init() {
    // No-op
}

/// Start HTTP server for profiling data (puffin only, port 8585)
#[cfg(feature = "profiling")]
pub fn start_server() {
    use std::sync::OnceLock;
    static SERVER: OnceLock<puffin_http::Server> = OnceLock::new();

    SERVER.get_or_init(|| {
        let server_addr = format!("0.0.0.0:{}", puffin_http::DEFAULT_PORT);
        let server = puffin_http::Server::new(&server_addr)
            .expect("Failed to start puffin HTTP server");

        tracing::info!("Puffin profiling server started on http://localhost:{}", puffin_http::DEFAULT_PORT);
        tracing::info!("View profiling data with puffin_viewer or browser");

        server
    });
}

/// Start profiling HTTP server (no-op when profiling disabled)
#[cfg(not(feature = "profiling"))]
pub fn start_server() {
    // No-op
}

/// Mark end of frame for profiling
#[cfg(feature = "profiling")]
pub fn finish_frame() {
    puffin::GlobalProfiler::lock().new_frame();
}

/// Finish frame (no-op when profiling disabled)
#[cfg(not(feature = "profiling"))]
pub fn finish_frame() {
    // No-op
}

/// Get profiling stats (scope_count, total_time_ns)
#[cfg(feature = "profiling")]
pub fn stats() -> (usize, u64) {
    let _profile = puffin::GlobalProfiler::lock();
    (0, 0) // Placeholder
}

/// Get profiling statistics (returns zeros when disabled)
#[cfg(not(feature = "profiling"))]
pub fn stats() -> (usize, u64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        // Just ensure it doesn't panic
        init();
    }

    #[test]
    fn test_start_server() {
        // Just ensure it doesn't panic
        start_server();
    }

    #[test]
    fn test_finish_frame() {
        // Just ensure it doesn't panic
        finish_frame();
    }

    #[test]
    fn test_stats() {
        let (count, time) = stats();
        // When profiling is disabled, should return zeros
        #[cfg(not(feature = "profiling"))]
        {
            assert_eq!(count, 0);
            assert_eq!(time, 0);
        }
    }

    #[test]
    fn test_profile_macros() {
        // Test that macros compile and don't panic
        profile_function!();
        profile_scope!("test_scope");

        let result = profile_expr!("test_expr", {
            42
        });
        assert_eq!(result, 42);
    }
}
