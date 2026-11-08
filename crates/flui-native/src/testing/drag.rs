//! Drag-transport scenarios every backend's harness can run.

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DragTransport, RepresentationIndex, TransferError, TransferLimits, TransferPayload,
    TransferRequest,
};

/// Ask `transport` directly for one representation and require its delivery to
/// fail the byte ceiling. `resolve` drives the backend's request to completion.
///
/// Calling the transport rather than a view keeps this check on the backend's
/// own contract, including for callers that do not add another enforcement
/// layer.
///
/// # Panics
///
/// Panics when the request resolves successfully or with another error, or
/// when its `TooLarge` measurements do not match `limits`.
pub fn oversized_delivery(
    transport: &dyn DragTransport,
    id: DataTransferId,
    representation: RepresentationIndex,
    limits: TransferLimits,
    resolve: impl FnOnce(TransferRequest) -> Result<TransferPayload, TransferError>,
) {
    let outcome = resolve(transport.request(id, representation, limits));
    assert_oversized(outcome, limits);
}

/// The asynchronous form of [`oversized_delivery`], for a backend whose source
/// resolves on its event loop. `resolve` drives and awaits that request.
///
/// # Panics
///
/// Panics under the same conditions as [`oversized_delivery`].
pub async fn oversized_delivery_async<F>(
    transport: &dyn DragTransport,
    id: DataTransferId,
    representation: RepresentationIndex,
    limits: TransferLimits,
    resolve: impl FnOnce(TransferRequest) -> F,
) where
    F: std::future::Future<Output = Result<TransferPayload, TransferError>>,
{
    let outcome = resolve(transport.request(id, representation, limits)).await;
    assert_oversized(outcome, limits);
}

fn assert_oversized(outcome: Result<TransferPayload, TransferError>, limits: TransferLimits) {
    match outcome {
        Err(TransferError::TooLarge { actual, limit }) => {
            assert!(
                actual > limits.max_bytes,
                "the reported size {actual} does not exceed the ceiling {}",
                limits.max_bytes
            );
            assert_eq!(limit, limits.max_bytes, "the request's byte ceiling");
        }
        other => panic!("an oversized delivery was not refused: {other:?}"),
    }
}
