### Fixed

- Serialize reentrant owner-thread dispatch across UI realms through one FIFO, so a sibling
  window's close or input callback is not lost while another realm is running.
