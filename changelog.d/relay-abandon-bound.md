### Fixed

- `flui run`'s page relay writes a client the lines the page sent before it ended the
  connection, across a reload and after a line the page refused, instead of closing the
  client with them unwritten.
- A connect call the page does not answer in time no longer counts as one that opened nothing:
  the relay closes its client and abandons the connection by its ID, which closes it if the page
  runs the call late. The abandon is retried a bounded number of times, and a page that never
  answers holds up no other client.
- A line the page reports for a connection after ending it is dropped instead of counting
  against the client's bound.
- The handle of a command the browser answers after the relay stopped waiting is released.
