//! The resource policy every drag backend applies to an untrusted offer and
//! its fetches. This is base transfer code: it names no drag or host type.

use std::time::Duration;

/// The common bounds on announced types and concurrent fetches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Intake {
    /// The most source entries read, including duplicates and refused names.
    pub(crate) max_types: usize,
    /// The longest admitted type name, in bytes.
    pub(crate) max_type_len: usize,
    /// The most payload fetches running at once.
    pub(crate) max_fetches: usize,
    /// How long a fetch waits for its source.
    pub(crate) fetch_timeout: Duration,
}

impl Intake {
    /// The policy every drag backend uses unless its protocol imposes an
    /// additional bound of its own.
    pub(crate) const DEFAULT: Self = Self {
        max_types: 100,
        max_type_len: 255,
        max_fetches: 4,
        fetch_timeout: Duration::from_secs(5),
    };

    /// Whether `candidate` may join `admitted`: its name is short enough and
    /// no admitted entry has the same name.
    pub(crate) fn admit_type<T>(
        &self,
        admitted: &[T],
        candidate: &T,
        name_of: impl Fn(&T) -> &str,
    ) -> bool {
        let name = name_of(candidate);
        name.len() <= self.max_type_len && admitted.iter().all(|held| name_of(held) != name)
    }

    /// Admit the entries of `types`, in source order. At most `max_types`
    /// source entries are pulled, even when duplicates or overlong names
    /// leave fewer admitted entries; an endless stream therefore cannot keep
    /// the filter reading.
    pub(crate) fn admit_types<T>(
        &self,
        types: impl IntoIterator<Item = T>,
        name_of: impl Fn(&T) -> &str,
    ) -> Vec<T> {
        let mut admitted = Vec::new();
        for candidate in types.into_iter().take(self.max_types) {
            if self.admit_type(&admitted, &candidate, &name_of) {
                admitted.push(candidate);
            }
        }
        admitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        name: &'static str,
        values: Vec<String>,
        endless: bool,
        expected: Vec<String>,
        reads: usize,
    }

    /// The default bounds hold at their edges, duplicates keep their first
    /// position, and the filter pulls no more source entries than its bound,
    /// even from an endless stream of one repeated name.
    #[test]
    fn admission_is_bounded_by_source_entries_and_name_length() {
        assert_eq!(Intake::DEFAULT.max_types, 100);
        assert_eq!(Intake::DEFAULT.max_type_len, 255);
        assert_eq!(Intake::DEFAULT.max_fetches, 4);
        assert_eq!(Intake::DEFAULT.fetch_timeout, Duration::from_secs(5));

        let unique = |count| {
            (0..count)
                .map(|n| format!("application/x-{n}"))
                .collect::<Vec<_>>()
        };
        let cases = [
            Case {
                name: "100 types",
                values: unique(100),
                endless: false,
                expected: unique(100),
                reads: 100,
            },
            Case {
                name: "101 types",
                values: unique(101),
                endless: false,
                expected: unique(100),
                reads: 100,
            },
            Case {
                name: "255-byte name",
                values: vec!["x".repeat(255)],
                endless: false,
                expected: vec!["x".repeat(255)],
                reads: 1,
            },
            Case {
                name: "256-byte name",
                values: vec!["x".repeat(256)],
                endless: false,
                expected: Vec::new(),
                reads: 1,
            },
            Case {
                name: "duplicates",
                values: ["text/plain", "text/plain", "text/html", "text/plain"]
                    .map(str::to_owned)
                    .into(),
                endless: false,
                expected: vec!["text/plain".to_owned(), "text/html".to_owned()],
                reads: 4,
            },
            Case {
                name: "endless duplicates do not overread",
                values: vec!["text/plain".to_owned()],
                endless: true,
                expected: vec!["text/plain".to_owned()],
                reads: 100,
            },
        ];
        for case in cases {
            let mut reads = 0;
            let input: Box<dyn Iterator<Item = String>> = if case.endless {
                Box::new(case.values.into_iter().cycle())
            } else {
                Box::new(case.values.into_iter())
            };
            let admitted =
                Intake::DEFAULT.admit_types(input.inspect(|_| reads += 1), String::as_str);
            assert_eq!(admitted, case.expected, "{}", case.name);
            assert_eq!(reads, case.reads, "{}: source entries read", case.name);
        }
    }
}
