//! Driving the acceptance application headless, the way its rows do: start
//! it over a storage, restart it over the same storage, close it or end its
//! session, and give it the user's taps and keys.

use std::time::Duration;

use flui::foundation::RenderId;
use flui::interaction::PointerKind;
use flui::platform::EventTime;
use flui::platform::keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui::testing::storage::MemoryStorage;
use flui::testing::widgets::{LaidOut, lay_out_with_storage, offset, settle_lazy, tight};
use flui::view::CloseReason;

use crate::app::AcceptanceApp;

/// The headless window the application runs in.
pub const WIDTH: f64 = 640.0;
pub const HEIGHT: f64 = 720.0;

/// Longer than any page transition, so a route change has finished.
const SETTLE_STEP: Duration = Duration::from_millis(50);
const SETTLE_STEPS: usize = 8;

/// One run of the application: its mounted tree, the storage it runs over and
/// the configuration a restart mounts again.
pub struct AppRun {
    laid: LaidOut,
    storage: MemoryStorage,
    app: AcceptanceApp,
}

impl AppRun {
    /// Start `app` over a fresh, empty storage.
    pub fn start(app: AcceptanceApp) -> Self {
        Self::start_over(app, MemoryStorage::new())
    }

    /// Start `app` over `storage`, as a launch over what earlier runs left.
    pub fn start_over(app: AcceptanceApp, storage: MemoryStorage) -> Self {
        let mut laid = lay_out_with_storage(app.clone(), tight(WIDTH, HEIGHT), storage.clone());
        laid.enable_semantics();
        let mut run = Self { laid, storage, app };
        run.settle();
        run
    }

    /// Stop this run and launch the same application over the same storage.
    /// The old tree is gone before the new one mounts, as across a process
    /// restart.
    #[must_use]
    pub fn restart(self) -> Self {
        let Self { laid, storage, app } = self;
        drop(laid);
        Self::start_over(app, storage)
    }

    /// The storage this run reads and writes.
    pub fn storage(&self) -> &MemoryStorage {
        &self.storage
    }

    /// The mounted tree, for input the methods here do not cover.
    pub fn laid(&mut self) -> &mut LaidOut {
        &mut self.laid
    }

    /// The user closes the window.
    pub fn request_close(&mut self) {
        self.laid.request_close(CloseReason::User);
        self.settle();
    }

    /// The operating system ends the session.
    pub fn end_session(&mut self) {
        self.laid.end_session();
        self.settle();
    }

    /// Let virtual time pass until loads, transitions and lazy rows are done.
    pub fn settle(&mut self) {
        for _ in 0..SETTLE_STEPS {
            self.laid.pump_for(SETTLE_STEP);
        }
        settle_lazy(&mut self.laid);
    }

    /// Whether `text` is on screen where a pointer reaches it: a covered or
    /// departed page's text is not.
    pub fn shows(&self, text: &str) -> bool {
        self.reachable(text).is_some()
    }

    /// Tap the on-screen `text`, then settle.
    ///
    /// # Panics
    ///
    /// When `text` is not on screen.
    pub fn tap(&mut self, text: &str) {
        let (x, y) = self
            .reachable(text)
            .unwrap_or_else(|| panic!("{text:?} is not on screen"));
        self.laid.dispatch_pointer_down(x, y);
        self.laid.dispatch_pointer_up(x, y);
        self.settle();
    }

    /// Focus the Title field and replace its text by typing `text`.
    ///
    /// # Panics
    ///
    /// When no Title field is on screen.
    pub fn replace_title(&mut self, text: &str) {
        let field = self.title_field();
        let position = self.laid.absolute_offset(field);
        let size = self.laid.size(field);
        let (x, y) = (position.dx + 2.0, position.dy + size.height / 2.0);
        self.laid.dispatch_pointer_down(x, y);
        self.laid.dispatch_pointer_up(x, y);
        self.settle();
        let command = if cfg!(any(target_os = "macos", target_os = "ios")) {
            Modifiers::META
        } else {
            Modifiers::CONTROL
        };
        self.press(Key::character("a"), command);
        self.press(Key::Named(NamedKey::Backspace), Modifiers::NONE);
        for character in text.chars() {
            self.press(Key::character(character.to_string()), Modifiers::NONE);
        }
        self.settle();
    }

    /// The text the on-screen Title field holds.
    pub fn title(&self) -> String {
        self.laid
            .render_property(self.title_field(), "text")
            .expect("BUG: an editable reports its text")
    }

    fn press(&self, key: Key, modifiers: Modifiers) {
        for state in [KeyState::Down, KeyState::Up] {
            self.laid.dispatch_key(
                KeyEvent::new(
                    state,
                    key.clone(),
                    Code::Unidentified,
                    EventTime::from_nanos(0),
                )
                .with_modifiers(modifiers),
            );
        }
    }

    fn title_field(&self) -> RenderId {
        let fields: Vec<_> = self
            .laid
            .find_all_by_render_type("RenderEditable")
            .into_iter()
            .filter(|field| self.centre_if_hit(*field).is_some())
            .collect();
        assert_eq!(fields.len(), 1, "one Title field on screen");
        fields[0]
    }

    /// The centre of a render object showing `text` that a pointer there
    /// reaches.
    fn reachable(&self, text: &str) -> Option<(f64, f64)> {
        self.laid
            .find_all_text(text)
            .into_iter()
            .find_map(|id| self.centre_if_hit(id))
    }

    fn centre_if_hit(&self, id: RenderId) -> Option<(f64, f64)> {
        let size = self.laid.try_size(id)?;
        if size.width <= 0.0 || size.height <= 0.0 {
            return None;
        }
        let position = self.laid.absolute_offset(id);
        let (x, y) = (
            position.dx + size.width / 2.0,
            position.dy + size.height / 2.0,
        );
        if !(0.0..WIDTH).contains(&x) || !(0.0..HEIGHT).contains(&y) {
            return None;
        }
        let path = self.laid.hit_test_pointer(offset(x, y), PointerKind::Mouse);
        path.path()
            .iter()
            .any(|entry| entry.target == id)
            .then_some((x, y))
    }
}
