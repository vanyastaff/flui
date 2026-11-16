### Fixed
- The `flui run --device browser:…` development server no longer answers 503 to a client that reconnects right after reading a full answer while the server is near its connection limit: a connection's place is given back before its socket closes.
