### Fixed

- The Windows development-agent connection yields while pipe input is actively arriving and parks only after 50 ms of idle. Large protocol lines no longer depend on the 10 ms idle polling cadence to make progress within the client's timeout.
