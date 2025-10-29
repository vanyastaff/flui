//! Network monitoring for HTTP requests and responses.
//!
//! This module provides tools for tracking and inspecting network traffic
//! in FLUI applications, similar to browser DevTools network panels.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

/// Represents an HTTP request being tracked.
#[derive(Debug, Clone)]
pub struct NetworkRequest {
    /// Unique identifier for this request
    pub id: u64,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Request URL
    pub url: String,
    /// Request headers
    pub headers: Vec<(String, String)>,
    /// Request body size in bytes
    pub body_size: usize,
    /// Timestamp when request started
    pub timestamp: std::time::Instant,
}

/// Represents an HTTP response.
#[derive(Debug, Clone)]
pub struct NetworkResponse {
    /// Request ID this response corresponds to
    pub request_id: u64,
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: Vec<(String, String)>,
    /// Response body size in bytes
    pub body_size: usize,
    /// Time taken to receive response
    pub duration: Duration,
}

/// Network monitor for tracking HTTP traffic.
///
/// # Example
///
/// ```rust,ignore
/// use flui_devtools::network::NetworkMonitor;
///
/// let mut monitor = NetworkMonitor::new();
/// monitor.start();
///
/// // Track requests and responses
/// let req_id = monitor.track_request("GET", "https://api.example.com/data");
/// // ... make request ...
/// monitor.track_response(req_id, 200, 1024);
///
/// // Get statistics
/// let stats = monitor.stats();
/// println!("Total requests: {}", stats.total_requests);
/// ```
#[derive(Debug)]
pub struct NetworkMonitor {
    requests: Vec<NetworkRequest>,
    responses: Vec<NetworkResponse>,
    next_id: u64,
    enabled: bool,
}

impl NetworkMonitor {
    /// Creates a new network monitor.
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
            responses: Vec::new(),
            next_id: 0,
            enabled: false,
        }
    }

    /// Starts monitoring network requests.
    pub fn start(&mut self) {
        self.enabled = true;
    }

    /// Stops monitoring network requests.
    pub fn stop(&mut self) {
        self.enabled = false;
    }

    /// Tracks a new network request.
    ///
    /// Returns the request ID that can be used to track the response.
    pub fn track_request(&mut self, method: impl Into<String>, url: impl Into<String>) -> u64 {
        if !self.enabled {
            return 0;
        }

        let id = self.next_id;
        self.next_id += 1;

        self.requests.push(NetworkRequest {
            id,
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body_size: 0,
            timestamp: std::time::Instant::now(),
        });

        id
    }

    /// Tracks a network response for a given request.
    pub fn track_response(&mut self, request_id: u64, status: u16, body_size: usize) {
        if !self.enabled {
            return;
        }

        if let Some(request) = self.requests.iter().find(|r| r.id == request_id) {
            self.responses.push(NetworkResponse {
                request_id,
                status,
                headers: Vec::new(),
                body_size,
                duration: request.timestamp.elapsed(),
            });
        }
    }

    /// Returns network statistics.
    pub fn stats(&self) -> NetworkStats {
        NetworkStats {
            total_requests: self.requests.len(),
            total_responses: self.responses.len(),
            total_bytes_sent: self.requests.iter().map(|r| r.body_size).sum(),
            total_bytes_received: self.responses.iter().map(|r| r.body_size).sum(),
        }
    }

    /// Clears all tracked requests and responses.
    pub fn clear(&mut self) {
        self.requests.clear();
        self.responses.clear();
    }
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Network statistics.
#[derive(Debug, Clone, Copy)]
pub struct NetworkStats {
    /// Total number of requests tracked
    pub total_requests: usize,
    /// Total number of responses received
    pub total_responses: usize,
    /// Total bytes sent in requests
    pub total_bytes_sent: usize,
    /// Total bytes received in responses
    pub total_bytes_received: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_monitor() {
        let mut monitor = NetworkMonitor::new();
        monitor.start();

        let req_id = monitor.track_request("GET", "https://example.com");
        monitor.track_response(req_id, 200, 1024);

        let stats = monitor.stats();
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.total_responses, 1);
    }
}
