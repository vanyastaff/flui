//! Window creation request system
//!
//! Handles window creation requests from outside the event loop.
//! Since winit 0.30 requires ActiveEventLoop to create windows,
//! we use channels to communicate between Platform methods and the event loop.

use crate::traits::{WindowId, WindowOptions};
use anyhow::Result;
use parking_lot::Mutex;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Request to create a new window
#[derive(Debug)]
pub struct WindowRequest {
    /// Window configuration
    pub options: WindowOptions,

    /// Channel to send the result back
    pub response: ResponseSender,
}

/// Response sender for window creation
pub type ResponseSender = std::sync::mpsc::SyncSender<Result<WindowId>>;

/// Window request queue
///
/// Thread-safe queue for window creation requests.
/// The Platform sends requests, the event loop processes them.
pub struct WindowRequestQueue {
    sender: Sender<WindowRequest>,
    receiver: Arc<Mutex<Receiver<WindowRequest>>>,
}

impl WindowRequestQueue {
    /// Create a new window request queue
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Get a sender for submitting window requests
    pub fn sender(&self) -> Sender<WindowRequest> {
        self.sender.clone()
    }

    /// Get all pending window requests
    ///
    /// This should be called from the event loop to process requests.
    pub fn drain_pending(&self) -> Vec<WindowRequest> {
        let receiver = self.receiver.lock();
        let mut requests = Vec::new();

        // Drain all pending requests without blocking
        while let Ok(request) = receiver.try_recv() {
            requests.push(request);
        }

        requests
    }
}

impl Default for WindowRequestQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::{px, Size};

    #[test]
    fn test_window_request_queue() {
        let queue = WindowRequestQueue::new();
        let sender = queue.sender();

        // Create a request
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
        let options = WindowOptions {
            title: "Test Window".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            ..Default::default()
        };

        let request = WindowRequest {
            options,
            response: response_tx,
        };

        // Send request
        sender.send(request).unwrap();

        // Drain requests
        let requests = queue.drain_pending();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].options.title, "Test Window");

        // Queue should be empty now
        let requests = queue.drain_pending();
        assert_eq!(requests.len(), 0);
    }

    #[test]
    fn test_multiple_requests() {
        let queue = WindowRequestQueue::new();
        let sender = queue.sender();

        // Send multiple requests
        for i in 0..5 {
            let (response_tx, _response_rx) = std::sync::mpsc::sync_channel(1);
            let options = WindowOptions {
                title: format!("Window {}", i),
                size: Size::new(px(800.0), px(600.0)),
                ..Default::default()
            };

            sender
                .send(WindowRequest {
                    options,
                    response: response_tx,
                })
                .unwrap();
        }

        // Drain all
        let requests = queue.drain_pending();
        assert_eq!(requests.len(), 5);
    }
}
