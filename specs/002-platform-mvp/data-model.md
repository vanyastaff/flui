# Data Model: flui-platform MVP Completion

**Date**: 2026-02-13 | **Branch**: `002-platform-mvp`

## Entity Relationship Overview

```text
Platform (singleton)
├── owns → BackgroundExecutor (1)
├── owns → ForegroundExecutor (1)
├── owns → Arc<dyn PlatformTextSystem> (1)
├── owns → Arc<dyn Clipboard> (1)  [→ will become ClipboardItem-based]
├── owns → PlatformHandlers (callbacks)
├── queries → Vec<Arc<dyn PlatformDisplay>> (N)
├── creates → Box<dyn PlatformWindow> (N)
│   ├── has → WindowCallbacks (per-window)
│   ├── has → WindowId
│   ├── queries → Arc<dyn PlatformDisplay> (associated)
│   └── provides → HasWindowHandle + HasDisplayHandle (for GPU)
└── provides → &dyn PlatformCapabilities

Task<T> (standalone)
├── wraps → tokio::task::JoinHandle<T>
└── implements → Future<Output = T>

DirectWriteTextSystem (Windows-specific)
├── owns → IDWriteFactory5
├── owns → IDWriteFontCollection1 (system)
├── owns → Vec<FontInfo> (loaded fonts)
└── implements → PlatformTextSystem
```

## Core Entities

### Platform (trait)

| Field/Method | Type | Description |
|---|---|---|
| `run()` | `Box<dyn FnOnce()>` → blocks | Starts event loop, calls on_ready |
| `quit()` | `()` | Exits event loop |
| `activate()` | `bool` → `()` | Bring app to foreground |
| `hide()` | `()` | Hide application |
| `open_window()` | `WindowOptions` → `Result<Box<dyn PlatformWindow>>` | Create native window |
| `active_window()` | → `Option<WindowId>` | Currently focused window |
| `displays()` | → `Vec<Arc<dyn PlatformDisplay>>` | All connected monitors |
| `set_cursor_style()` | `CursorStyle` → `()` | Change system cursor |
| `window_appearance()` | → `WindowAppearance` | System theme (light/dark) |
| `open_url()` | `&str` → `()` | Open URL in default browser |
| `prompt_for_paths()` | `PathPromptOptions` → `Task<Result<...>>` | Native file dialog |
| `keyboard_layout()` | → `String` | Current keyboard layout ID |
| `write_to_clipboard()` | `ClipboardItem` → `()` | Write to system clipboard |
| `read_from_clipboard()` | → `Option<ClipboardItem>` | Read from system clipboard |
| `background_executor()` | → `BackgroundExecutor` | Thread pool executor |
| `foreground_executor()` | → `ForegroundExecutor` | Main thread executor |
| `text_system()` | → `Arc<dyn PlatformTextSystem>` | Text/font services |
| `on_quit()` | callback → `()` | Register quit handler |
| `on_window_event()` | callback → `()` | Register window event handler |
| `on_open_urls()` | callback → `()` | Register URL handler |
| `on_keyboard_layout_change()` | callback → `()` | Register layout change handler |

### PlatformWindow (trait)

**State query methods:**

| Method | Return Type | Description |
|---|---|---|
| `physical_size()` | `Size<DevicePixels>` | Window size in device pixels |
| `logical_size()` | `Size<Pixels>` | Window size in logical pixels |
| `bounds()` | `Bounds<Pixels>` | Window position + size |
| `content_size()` | `Size<Pixels>` | Client area size |
| `window_bounds()` | `WindowBounds` | Windowed/Maximized/Fullscreen with bounds |
| `scale_factor()` | `f32` | DPI scale factor |
| `is_focused()` | `bool` | Has keyboard focus |
| `is_visible()` | `bool` | Is window visible |
| `is_maximized()` | `bool` | Is maximized |
| `is_fullscreen()` | `bool` | Is fullscreen |
| `is_active()` | `bool` | Is the active window |
| `is_hovered()` | `bool` | Mouse is over window |
| `mouse_position()` | `Point<Pixels>` | Cursor in window-local logical coords |
| `modifiers()` | `Modifiers` | Current keyboard modifiers |
| `appearance()` | `WindowAppearance` | Window theme |
| `display()` | `Option<Arc<dyn PlatformDisplay>>` | Associated monitor |
| `get_title()` | `String` | Current window title |

**Control methods:**

| Method | Parameters | Description |
|---|---|---|
| `set_title()` | `&str` | Set title bar text |
| `activate()` | `()` | Bring window to front |
| `minimize()` | `()` | Minimize to taskbar |
| `maximize()` | `()` | Maximize to screen |
| `restore()` | `()` | Restore from min/max |
| `toggle_fullscreen()` | `()` | Toggle fullscreen |
| `resize()` | `Size<Pixels>` | Resize window |
| `close()` | `()` | Close window |
| `request_redraw()` | `()` | Request repaint |
| `set_background_appearance()` | `WindowBackgroundAppearance` | Set backdrop |

**Callback registration methods:**

| Method | Callback Signature | Invoked When |
|---|---|---|
| `on_input()` | `FnMut(PlatformInput) -> DispatchEventResult` | Mouse/keyboard event |
| `on_request_frame()` | `FnMut()` | Frame should be rendered |
| `on_resize()` | `FnMut(Size<Pixels>, f32)` | Window resized |
| `on_moved()` | `FnMut()` | Window moved |
| `on_close()` | `FnOnce()` | Window destroyed |
| `on_should_close()` | `FnMut() -> bool` | Close requested (veto-able) |
| `on_active_status_change()` | `FnMut(bool)` | Focus gained/lost |
| `on_hover_status_change()` | `FnMut(bool)` | Mouse enter/leave |
| `on_appearance_changed()` | `FnMut()` | Theme changed |

### Task<T>

| Field | Type | Description |
|---|---|---|
| `state` | `TaskState<T>` | Ready(Option<T>) or Spawned(JoinHandle<T>) |

| Method | Signature | Description |
|---|---|---|
| `ready()` | `T → Task<T>` | Create completed task |
| `spawn()` | `Future<Output=T> → Task<T>` | Spawn on background |
| `detach()` | `self → ()` | Fire and forget |
| `poll()` | `Pin<&mut Self>, &mut Context → Poll<T>` | Future impl |

### PlatformTextSystem (trait)

| Method | Signature | Description |
|---|---|---|
| `add_fonts()` | `Vec<Cow<'static, [u8]>> → Result<()>` | Load font bytes |
| `all_font_names()` | → `Vec<String>` | Enumerate system fonts |
| `font_id()` | `&Font → Result<FontId>` | Resolve font descriptor to ID |
| `font_metrics()` | `FontId → FontMetrics` | Get font metrics |
| `glyph_for_char()` | `FontId, char → Option<GlyphId>` | Map character to glyph |
| `layout_line()` | `&str, f32, &[FontRun] → LineLayout` | Layout text line |

### WindowCallbacks (internal struct, per-window)

| Field | Type | Storage |
|---|---|---|
| `on_input` | `Mutex<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>>>` | parking_lot::Mutex |
| `on_request_frame` | `Mutex<Option<Box<dyn FnMut() + Send>>>` | parking_lot::Mutex |
| `on_resize` | `Mutex<Option<Box<dyn FnMut(Size<Pixels>, f32) + Send>>>` | parking_lot::Mutex |
| `on_moved` | `Mutex<Option<Box<dyn FnMut() + Send>>>` | parking_lot::Mutex |
| `on_close` | `Mutex<Option<Box<dyn FnOnce() + Send>>>` | parking_lot::Mutex |
| `on_should_close` | `Mutex<Option<Box<dyn FnMut() -> bool + Send>>>` | parking_lot::Mutex |
| `on_active_status_change` | `Mutex<Option<Box<dyn FnMut(bool) + Send>>>` | parking_lot::Mutex |
| `on_hover_status_change` | `Mutex<Option<Box<dyn FnMut(bool) + Send>>>` | parking_lot::Mutex |
| `on_appearance_changed` | `Mutex<Option<Box<dyn FnMut() + Send>>>` | parking_lot::Mutex |

## Value Types (enums/structs)

### CursorStyle
```
Arrow | IBeam | Crosshair | ClosedHand | OpenHand | PointingHand
| ResizeLeft | ResizeRight | ResizeLeftRight | ResizeUp | ResizeDown
| ResizeUpDown | ResizeUpLeftDownRight | ResizeUpRightDownLeft
| ResizeColumn | ResizeRow | OperationNotAllowed
| DragLink | DragCopy | ContextualMenu | None
```

### WindowAppearance
```
Light (default) | Dark | VibrantLight | VibrantDark
```

### WindowBackgroundAppearance
```
Opaque (default) | Transparent | Blurred | MicaBackdrop | MicaAltBackdrop
```

### WindowBounds
```
Windowed(Bounds<Pixels>) | Maximized(Bounds<Pixels>) | Fullscreen(Bounds<Pixels>)
```

### DispatchEventResult
```
{ propagate: bool, default_prevented: bool }
Default: { propagate: true, default_prevented: false }
```

### ClipboardItem
```
{ entries: Vec<ClipboardEntry> }
```

### ClipboardEntry
```
String(ClipboardString)
```

### PathPromptOptions
```
{ files: bool, directories: bool, multiple: bool }
```

### FontMetrics
```
{ units_per_em: u16, ascent: f32, descent: f32, line_gap: f32,
  underline_position: f32, underline_thickness: f32,
  cap_height: f32, x_height: f32 }
```

### Priority
```
High | Medium (default) | Low
```

## State Transitions

### Window Lifecycle
```
Created → Active → [Minimized ↔ Active ↔ Maximized ↔ Fullscreen] → CloseRequested → Closed
                                                                          ↑
                                                                    on_should_close() = false → stays Active
```

### Task Lifecycle
```
Spawned → Running → Completed (poll returns Ready)
                  → Detached (handle dropped, task continues)
    OR
Ready → Completed (immediate, single poll)
```

### Platform Lifecycle
```
new() → run(on_ready) → [event loop: dispatch events, run callbacks] → quit() → dropped
```
