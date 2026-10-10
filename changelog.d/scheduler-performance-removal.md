### Removed

- Removed the scheduler's ineffective `PerformanceMode` and `PerformanceModeRequestHandle` API, request counters and mode accessors. Callers should remove requests and handle bookkeeping; these hints never changed scheduling or power behavior. Application configuration that serialized the removed enum needs an application-owned representation. See the scheduler README for migration guidance.
