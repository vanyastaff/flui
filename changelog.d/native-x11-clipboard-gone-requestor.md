### Fixed

- The X11 clipboard never answers a client whose window went while its request for the primary selection waited on the application: the answer, or its refusal, used to reach whichever window took the gone one's id next, as the server hands a gone client's ids to the next one. A waiting request now hears its requestor's window and is dropped as it goes, and answers decided later are written only after the events read before them are handled.
