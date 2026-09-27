### Fixed

- A panicking post-frame callback no longer drops uninvoked callbacks: callers that recover may resume them in registration order on a later frame, without retrying the failed callback.
