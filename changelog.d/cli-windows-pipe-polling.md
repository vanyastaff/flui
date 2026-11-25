### Fixed

- On Windows, `flui run --agent`'s run endpoint (`run.reload`, which `flui mcp`'s `flui_reload`
  asks) and the relay that carries `flui mcp` to an app in a browser page no longer drop each
  client on its first quiet poll: a non-blocking named pipe answers an empty read as it answers a
  closed one, and both endpoints now tell the two apart. A reply that finds the pipe's buffer
  full waits for the client to read instead of closing the connection.
