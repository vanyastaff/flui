### Fixed

- Proxy animations release their parent lock before querying a custom animation, allowing a parent query to replace the proxy parent without deadlocking.
