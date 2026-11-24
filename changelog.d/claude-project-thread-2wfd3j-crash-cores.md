### Fixed

- A device rebuilt after a loss no longer races the lost device's pipeline warm-up: the request
  for the new device waits for the warm-up first. On Vulkan with wgpu's debug object names, the
  warm-up naming a pipeline while another thread made a device could crash the process.
