### Fixed
- Signal reads and updates retain released loan values after a callback failure, preventing aggregate destructors or nested release obligations from aborting recovery; ordinary released-value retirement still reports its first failure.
