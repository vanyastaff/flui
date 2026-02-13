//! Platform trait contract — the complete API surface for flui-platform MVP.
//!
//! This file defines the target trait signatures. It is NOT compiled code —
//! it is a design contract for the implementation phase.

use std::path::{Path, PathBuf};
use std::sync::Arc;

// --- Core Platform Trait ---

pub trait Platform: Send + Sync + 'static {
    // === Executors ===
    fn background_executor(&self) -> BackgroundExecutor;
    fn foreground_executor(&self) -> ForegroundExecutor;

    // === Text System ===
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;

    // === Lifecycle ===
    fn run(&self, on_ready: Box<dyn FnOnce()>);
    fn quit(&self);
    fn request_frame(&self);

    // === App Activation (NEW) ===
    fn activate(&self, ignoring_other_apps: bool) { /* default: no-op */
    }
    fn hide(&self) { /* default: no-op */
    }
    fn hide_other_apps(&self) { /* default: no-op */
    }
    fn unhide_other_apps(&self) { /* default: no-op */
    }

    // === Window Management ===
    fn open_window(&self, options: WindowOptions) -> anyhow::Result<Box<dyn PlatformWindow>>;
    fn active_window(&self) -> Option<WindowId>;
    fn window_stack(&self) -> Option<Vec<WindowId>> {
        None
    }

    // === Display Management ===
    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>>;
    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>>;

    // === Appearance (NEW) ===
    fn window_appearance(&self) -> WindowAppearance;
    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }

    // === Cursor (NEW) ===
    fn set_cursor_style(&self, style: CursorStyle);

    // === Clipboard (ENHANCED) ===
    fn write_to_clipboard(&self, item: ClipboardItem);
    fn read_from_clipboard(&self) -> Option<ClipboardItem>;

    // === File Operations (NEW) ===
    fn open_url(&self, url: &str);
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<anyhow::Result<Option<Vec<PathBuf>>>>;
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<anyhow::Result<Option<PathBuf>>>;
    fn reveal_path(&self, path: &Path) { /* default: no-op */
    }
    fn open_with_system(&self, path: &Path) { /* default: no-op */
    }

    // === Keyboard (NEW) ===
    fn keyboard_layout(&self) -> String;
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>);

    // === Callbacks (existing + enhanced) ===
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);
    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) { /* default: no-op */
    }
    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>);
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>) { /* default: no-op */
    }

    // === Platform Info ===
    fn capabilities(&self) -> &dyn PlatformCapabilities;
    fn name(&self) -> &'static str;
    fn compositor_name(&self) -> &'static str {
        ""
    }
    fn app_path(&self) -> anyhow::Result<PathBuf>;
}
