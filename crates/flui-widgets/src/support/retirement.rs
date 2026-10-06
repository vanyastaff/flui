//! Takeable terminal ownership for widget infrastructure.
//! Owners withdraw every outgoing slot before retiring any; collection tails
//! survive the first failure. User aggregates remain opaque.

/// A takeable terminal ownership slot. Ordinary retirement preserves normal
/// destruction; incoming unwind retains the opaque value instead of risking a
/// second destructor failure. Owners withdraw all slots before retiring any.
/// No panic is caught here: the first failure remains authoritative. A user
/// aggregate with multiple internally panicking fields remains opaque.
pub(crate) struct Terminal<T>(Option<T>);

impl<T: std::fmt::Debug> std::fmt::Debug for Terminal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(value) => value.fmt(f),
            None => f.write_str("<retired>"),
        }
    }
}

impl<T> Terminal<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Some(value))
    }
    pub(crate) fn withdraw(&mut self) -> Self {
        Self(self.0.take())
    }
    pub(crate) fn take_value(&mut self) -> T {
        self.0.take().expect("BUG: terminal ownership taken twice")
    }
}

impl<T: Default> Default for Terminal<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> std::ops::Deref for Terminal<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
            .as_ref()
            .expect("BUG: terminal ownership accessed after withdrawal")
    }
}

impl<T> std::ops::DerefMut for Terminal<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0
            .as_mut()
            .expect("BUG: terminal ownership accessed after withdrawal")
    }
}

impl<T> Drop for Terminal<T> {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            if std::thread::panicking() {
                std::mem::forget(value);
            } else {
                drop(value);
            }
        }
    }
}

/// The vector has left its owner/guard. Retain the iterator's entire tail if
/// one member fails, rather than unwind-dropping the remaining members.
pub(crate) fn retire_values<T>(values: Vec<T>) {
    let mut remaining = Terminal::new(values.into_iter());
    for value in remaining.by_ref() {
        drop(Terminal::new(value));
    }
}

pub(crate) struct RetiredValues<T>(pub(crate) Vec<T>);

impl<T> Drop for RetiredValues<T> {
    fn drop(&mut self) {
        retire_values(std::mem::take(&mut self.0));
    }
}

pub(crate) struct RetiredMap<K, V>(pub(crate) std::collections::HashMap<K, V>);

impl<K, V> Drop for RetiredMap<K, V> {
    fn drop(&mut self) {
        retire_map(std::mem::take(&mut self.0));
    }
}

pub(crate) fn retire_map<K, V>(values: std::collections::HashMap<K, V>) {
    let mut remaining = Terminal::new(values.into_iter());
    for (key, value) in remaining.by_ref() {
        // HeroTag keys can own user values. Secure both before retiring either.
        let key = Terminal::new(key);
        let value = Terminal::new(value);
        drop(key);
        drop(value);
    }
}

/// A shared map empties itself only at its physical last release, preserving
/// independently owned aliases until then.
#[derive(Debug)]
pub(crate) struct TerminalMap<K, V>(parking_lot::Mutex<std::collections::HashMap<K, V>>);

impl<K, V> Default for TerminalMap<K, V> {
    fn default() -> Self {
        Self::new(std::collections::HashMap::new())
    }
}

impl<K, V> TerminalMap<K, V> {
    pub(crate) fn new(values: std::collections::HashMap<K, V>) -> Self {
        Self(parking_lot::Mutex::new(values))
    }
}

impl<K, V> std::ops::Deref for TerminalMap<K, V> {
    type Target = parking_lot::Mutex<std::collections::HashMap<K, V>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> Drop for TerminalMap<K, V> {
    fn drop(&mut self) {
        retire_map(std::mem::take(self.0.get_mut()));
    }
}

#[derive(Debug)]
pub(crate) struct TerminalVec<T>(parking_lot::Mutex<Vec<T>>);

impl<T> Default for TerminalVec<T> {
    fn default() -> Self {
        Self(parking_lot::Mutex::new(Vec::new()))
    }
}

impl<T> std::ops::Deref for TerminalVec<T> {
    type Target = parking_lot::Mutex<Vec<T>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Drop for TerminalVec<T> {
    fn drop(&mut self) {
        retire_values(std::mem::take(self.0.get_mut()));
    }
}
