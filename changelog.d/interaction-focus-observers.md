### Fixed

- Focus notification rounds now deliver each still-registered node and manager observer after a callback panic, then propagate the first failure.
- Accepted reentrant focus requests now drain in FIFO order before the first observer failure propagates.
- Canceled queued focus targets and drain diagnostics preserve the first failure and the remaining accepted requests.
