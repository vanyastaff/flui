### Fixed

- Windows closes now release platform tracking after native context retirement, including callbacks that release their last external window owner.
- Windows getters observe native client bounds and updated event state before callbacks; hidden undecorated windows respect `visible: false`.
