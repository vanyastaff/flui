### Added

- Windows: a window takes `WindowFrame::AppTitleBar`. The caption goes and the side and bottom borders stay the system's, and the system's hit test is answered from the chrome regions the application marks (`WindowTitleBar`, `WindowCaptionButton`, `WindowResizeArea`): the bar moves the window with the system's snapping and window menu, a band along the top resizes it, and the maximize button shows the Windows 11 snap layouts while its presses reach the application's button. A `WindowFrame::None` window is moved and resized from the regions it marks.
