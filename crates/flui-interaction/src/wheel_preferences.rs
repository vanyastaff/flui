//! The owner-local wheel projection of the accepted host preferences.

use std::{cell::RefCell, rc::Rc};

use flui_platform_api::WheelPreferences;

/// Presentation-owned writer of the host's wheel projection.
///
/// Replacement invokes no user code. Consumers receive a read-only provider,
/// and resolve its current observations when accepting an input packet.
#[derive(Debug)]
pub struct WheelPreferencesSource {
    preferences: Rc<RefCell<WheelPreferences>>,
}

impl WheelPreferencesSource {
    /// Seed the projection with accepted observations.
    #[must_use]
    pub fn new(preferences: WheelPreferences) -> Self {
        Self {
            preferences: Rc::new(RefCell::new(preferences)),
        }
    }

    /// A read-only handle to this exact presentation's projection.
    #[must_use]
    pub fn provider(&self) -> WheelPreferencesProvider {
        WheelPreferencesProvider {
            profile: WheelProfile::Live(Rc::clone(&self.preferences)),
        }
    }

    /// Commit observations, returning whether their value changed.
    pub fn replace(&self, preferences: WheelPreferences) -> bool {
        let mut current = self.preferences.borrow_mut();
        if *current == preferences {
            return false;
        }
        *current = preferences;
        true
    }
}

/// Read-only wheel observations, either authored or presentation-owned.
///
/// Fixed values are independent of host updates. Live handles compare their
/// source identities and retain the last accepted observations if the writer
/// retires. A snapshot releases its borrow before any input callback runs.
#[derive(Debug, Clone, PartialEq)]
pub struct WheelPreferencesProvider {
    profile: WheelProfile,
}

#[derive(Debug, Clone)]
enum WheelProfile {
    Fixed(WheelPreferences),
    Live(Rc<RefCell<WheelPreferences>>),
}

impl PartialEq for WheelProfile {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed(left), Self::Fixed(right)) => left == right,
            (Self::Live(left), Self::Live(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl WheelPreferencesProvider {
    /// Copy observations without extending a borrow across user code.
    #[must_use]
    pub fn snapshot(&self) -> WheelPreferences {
        match &self.profile {
            WheelProfile::Fixed(preferences) => preferences.clone(),
            WheelProfile::Live(preferences) => preferences.borrow().clone(),
        }
    }
}

impl From<WheelPreferences> for WheelPreferencesProvider {
    fn from(preferences: WheelPreferences) -> Self {
        Self {
            profile: WheelProfile::Fixed(preferences),
        }
    }
}

impl Default for WheelPreferencesProvider {
    fn default() -> Self {
        WheelPreferences::default().into()
    }
}
