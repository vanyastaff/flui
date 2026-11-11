### Changed

- A frame request for external build work is a ticket from an `IdMint` under the inbox's lock instead of a shared atomic flag per request; a request made after the mint is exhausted stays owed and every later wake retries it: a label change allocates 390 times over four frames instead of 406, a full reassemble 1714 instead of 1840, and a `RawButton` 37 times instead of 39.
