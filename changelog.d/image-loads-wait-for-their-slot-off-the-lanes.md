### Fixed

- An image load waiting for one of the registry's load slots is no longer spawned on the IO lane until the slot is free, so more than a handful of cold image loads no longer fill the IO lane's admission window and make other IO work answer `SpawnError::Saturated`. Lanes with no room when the slot comes free hand the load to a request still waiting for it.
