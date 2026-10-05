### Fixed

- Pointer route cleanup permits callback captures to reenter the router during destruction and preserves routes registered by that cleanup for the next contact.
- Bulk pointer and global route cleanup, including final router destruction, retires callbacks independently, preserving the first retirement failure and retaining remaining captures during failure or an active unwind.
