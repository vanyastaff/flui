### Fixed

- `flui run`'s page relay writes a client the lines the page sent before it ended the
  connection, across a reload and after a line the page refused, instead of closing the
  client with them unwritten.
- A connect call the page does not answer in time no longer counts as one that opened nothing:
  the relay closes its client and abandons the connection by its ID, which closes it if the page
  runs the call late. Each ID is tried at most five times, each try waiting at most 300 ms for
  the page, so a page that answers nothing stalls the relay's other clients for that long per try
  instead of ten seconds, and only until the tries run out.
- A line the page reports for a connection after ending it is dropped instead of counting
  against the client's bound.
- The page object of a command the browser answers after the relay stopped waiting is released,
  as far as the wait for the tab's next event allows.
