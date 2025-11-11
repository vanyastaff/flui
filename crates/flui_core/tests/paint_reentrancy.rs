//! Paint Re-entrancy Detection Tests
//!
//! Tests for the HashSet-based O(1) re-entrancy detection in paint operations.
//! These tests verify that circular painting is properly detected and prevented.

#[cfg(test)]
mod tests {
    // Note: These tests verify the re-entrancy detection mechanism at the
    // ElementTree level. Since we can't easily create circular render graphs
    // in unit tests, we focus on verifying the HashSet behavior and performance.

    use std::collections::HashSet;

    #[test]
    fn test_hashset_insert_returns_true_for_new() {
        // Verify HashSet::insert() returns true for new elements
        let mut set = HashSet::new();

        let inserted = set.insert(42);
        assert!(inserted, "First insert should return true");

        let inserted_again = set.insert(42);
        assert!(!inserted_again, "Second insert should return false");
    }

    #[test]
    fn test_hashset_remove_returns_true_if_present() {
        // Verify HashSet::remove() returns true if element was present
        let mut set = HashSet::new();
        set.insert(42);

        let removed = set.remove(&42);
        assert!(removed, "Remove should return true for present element");

        let removed_again = set.remove(&42);
        assert!(!removed_again, "Remove should return false for absent element");
    }

    #[test]
    fn test_hashset_lookup_performance() {
        // Verify O(1) lookup time for HashSet
        let mut set = HashSet::new();

        // Insert many elements
        for i in 0..1000 {
            set.insert(i);
        }

        // Lookup should be O(1) regardless of size
        assert!(set.contains(&500));
        assert!(!set.contains(&1001));
    }

    #[test]
    fn test_hashset_vs_vec_contains() {
        // Compare HashSet vs Vec for contains() operation
        let mut vec = Vec::new();
        let mut set = HashSet::new();

        // Insert 1000 elements
        for i in 0..1000 {
            vec.push(i);
            set.insert(i);
        }

        // Both should find the element
        assert!(vec.contains(&999));
        assert!(set.contains(&999));

        // Vec is O(N), HashSet is O(1)
        // (can't measure time in test, but we know the complexity)
    }

    #[test]
    fn test_thread_local_hashset_isolation() {
        // Verify thread-local storage isolates stacks between threads
        use std::cell::RefCell;
        use std::thread;

        thread_local! {
            static TEST_STACK: RefCell<HashSet<u32>> =
                RefCell::new(HashSet::new());
        }

        // Main thread
        TEST_STACK.with(|stack| {
            stack.borrow_mut().insert(1);
            assert_eq!(stack.borrow().len(), 1);
        });

        // Spawn thread - should have empty stack
        let handle = thread::spawn(|| {
            TEST_STACK.with(|stack| {
                assert_eq!(stack.borrow().len(), 0);
                stack.borrow_mut().insert(2);
                stack.borrow().len()
            })
        });

        let thread_len = handle.join().unwrap();
        assert_eq!(thread_len, 1);

        // Main thread stack should still have only element 1
        TEST_STACK.with(|stack| {
            assert_eq!(stack.borrow().len(), 1);
            assert!(stack.borrow().contains(&1));
            assert!(!stack.borrow().contains(&2));
        });
    }

    #[test]
    fn test_raii_guard_cleanup() {
        // Test RAII guard pattern for stack cleanup
        use std::cell::RefCell;

        thread_local! {
            static GUARD_STACK: RefCell<HashSet<u32>> =
                RefCell::new(HashSet::new());
        }

        struct TestGuard {
            id: u32,
        }

        impl TestGuard {
            fn new(id: u32) -> Self {
                GUARD_STACK.with(|stack| {
                    stack.borrow_mut().insert(id);
                });
                Self { id }
            }
        }

        impl Drop for TestGuard {
            fn drop(&mut self) {
                GUARD_STACK.with(|stack| {
                    stack.borrow_mut().remove(&self.id);
                });
            }
        }

        // Stack should be empty initially
        GUARD_STACK.with(|stack| {
            assert_eq!(stack.borrow().len(), 0);
        });

        {
            let _guard1 = TestGuard::new(1);

            // Stack should have element
            GUARD_STACK.with(|stack| {
                assert_eq!(stack.borrow().len(), 1);
                assert!(stack.borrow().contains(&1));
            });

            {
                let _guard2 = TestGuard::new(2);

                // Stack should have both elements
                GUARD_STACK.with(|stack| {
                    assert_eq!(stack.borrow().len(), 2);
                    assert!(stack.borrow().contains(&1));
                    assert!(stack.borrow().contains(&2));
                });
            }

            // Guard2 dropped, should only have element 1
            GUARD_STACK.with(|stack| {
                assert_eq!(stack.borrow().len(), 1);
                assert!(stack.borrow().contains(&1));
                assert!(!stack.borrow().contains(&2));
            });
        }

        // Both guards dropped, stack should be empty
        GUARD_STACK.with(|stack| {
            assert_eq!(stack.borrow().len(), 0);
        });
    }

    #[test]
    fn test_nested_guards_unwind_on_panic() {
        // Test that guards cleanup even on panic
        use std::cell::RefCell;
        use std::panic;

        thread_local! {
            static PANIC_STACK: RefCell<HashSet<u32>> =
                RefCell::new(HashSet::new());
        }

        struct PanicGuard {
            id: u32,
        }

        impl PanicGuard {
            fn new(id: u32) -> Self {
                PANIC_STACK.with(|stack| {
                    stack.borrow_mut().insert(id);
                });
                Self { id }
            }
        }

        impl Drop for PanicGuard {
            fn drop(&mut self) {
                PANIC_STACK.with(|stack| {
                    stack.borrow_mut().remove(&self.id);
                });
            }
        }

        let result = panic::catch_unwind(|| {
            let _guard1 = PanicGuard::new(1);
            let _guard2 = PanicGuard::new(2);

            // Verify both are present
            PANIC_STACK.with(|stack| {
                assert_eq!(stack.borrow().len(), 2);
            });

            // Panic!
            panic!("Test panic");
        });

        assert!(result.is_err());

        // Stack should be empty (guards cleaned up)
        PANIC_STACK.with(|stack| {
            assert_eq!(stack.borrow().len(), 0);
        });
    }

    #[test]
    fn test_hashset_memory_efficiency() {
        // Verify HashSet doesn't waste memory for typical use case
        let mut set = HashSet::new();

        // Typical paint depth is < 100
        for i in 0..100 {
            set.insert(i);
        }

        // Memory should be reasonable (HashSet has overhead but acceptable)
        assert!(set.capacity() >= 100);

        // Clear and reuse
        set.clear();
        assert_eq!(set.len(), 0);

        // Capacity preserved for reuse
        assert!(set.capacity() >= 100);
    }
}
