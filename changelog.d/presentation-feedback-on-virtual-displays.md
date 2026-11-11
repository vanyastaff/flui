### Fixed

- A Win32 or AppKit window no longer claims presentation feedback on a machine whose display never
  tells when a frame was shown: a Windows desktop driven only by the Microsoft Basic Render Driver
  (a Hyper-V virtual machine) and a Mac that is a hypervisor's guest now answer
  `WindowCapabilities::presentation_feedback` with `NotOnThisDevice`, where their frame requests
  carried no report though the window said `Native`.
