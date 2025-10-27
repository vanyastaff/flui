//! Layout Cache Demo
//!
//! This example demonstrates the layout cache with statistics tracking.

use flui_core::render::cache::{LayoutCache, LayoutCacheKey, LayoutResult};
use flui_types::{Size, constraints::BoxConstraints};

fn main() {
    println!("=== LayoutCache Demo ===\n");

    // Create a new cache for demonstration
    let cache = LayoutCache::new();

    println!("1. Initial state:");
    print_cache_stats(&cache);

    // Simulate layout caching
    println!("\n2. Caching some layout results...");

    let key1 = LayoutCacheKey::new(1, BoxConstraints::tight(Size::new(100.0, 100.0)));
    let key2 = LayoutCacheKey::new(2, BoxConstraints::tight(Size::new(200.0, 150.0)));
    let key3 = LayoutCacheKey::new(3, BoxConstraints::tight(Size::new(300.0, 200.0)))
        .with_child_count(5);

    cache.insert(key1, LayoutResult::new(Size::new(100.0, 100.0)));
    cache.insert(key2, LayoutResult::new(Size::new(200.0, 150.0)));
    cache.insert(key3, LayoutResult::new(Size::new(300.0, 200.0)));

    println!("   Inserted 3 layout results");
    println!("   Cache entries: {}", cache.entry_count());

    // Test cache hits
    println!("\n3. Testing cache hits:");

    if let Some(result) = cache.get(&key1) {
        println!("   ✓ Cache HIT for key1: {:?}", result.size);
    }

    if let Some(result) = cache.get(&key2) {
        println!("   ✓ Cache HIT for key2: {:?}", result.size);
    }

    if let Some(result) = cache.get(&key3) {
        println!("   ✓ Cache HIT for key3 (multi-child): {:?}", result.size);
    }

    print_cache_stats(&cache);

    // Test cache misses
    println!("\n4. Testing cache misses:");

    let missing_key = LayoutCacheKey::new(999, BoxConstraints::tight(Size::ZERO));

    for i in 0..3 {
        if cache.get(&missing_key).is_none() {
            println!("   ✗ Cache MISS {} for non-existent key", i + 1);
        }
    }

    print_cache_stats(&cache);

    // Test invalidation
    println!("\n5. Testing cache invalidation:");
    println!("   Invalidating key1...");
    cache.invalidate(&key1);

    if cache.get(&key1).is_none() {
        println!("   ✗ Cache MISS after invalidation (expected)");
    }

    print_cache_stats(&cache);

    // Demonstrate statistics reset
    println!("\n6. Resetting statistics:");
    cache.reset_stats();
    print_cache_stats(&cache);

    println!("\n7. Using cache after reset:");
    cache.get(&key2);
    cache.get(&key3);
    print_cache_stats(&cache);

    // Test multi-child cache key behavior
    println!("\n8. Multi-child cache key demonstration:");
    let base_key = LayoutCacheKey::new(100, BoxConstraints::tight(Size::new(500.0, 300.0)));
    let multi_key_1 = base_key.with_child_count(3);
    let multi_key_2 = base_key.with_child_count(5);

    cache.insert(multi_key_1, LayoutResult::new(Size::new(500.0, 300.0)));
    cache.insert(multi_key_2, LayoutResult::new(Size::new(500.0, 350.0)));

    println!("   Cached same element with 3 and 5 children");

    if cache.get(&multi_key_1).is_some() {
        println!("   ✓ HIT: Layout with 3 children");
    }

    if cache.get(&multi_key_2).is_some() {
        println!("   ✓ HIT: Layout with 5 children");
    }

    // Same element, no child count - should miss
    if cache.get(&base_key).is_none() {
        println!("   ✗ MISS: Same element without child count (different key)");
    }

    print_cache_stats(&cache);

    // Clear cache
    println!("\n9. Clearing entire cache:");
    cache.clear();
    println!("   Entries after clear: {}", cache.entry_count());
    print_cache_stats(&cache);

    // Debug format demonstration
    println!("\n10. Debug format output:");
    println!("   {:?}", cache);

    println!("\n=== Demo Complete ===");
}

fn print_cache_stats(cache: &LayoutCache) {
    let (hits, misses, total, hit_rate) = cache.detailed_stats();
    println!("   Statistics:");
    println!("     Entries: {}", cache.entry_count());
    println!("     Hits: {}", hits);
    println!("     Misses: {}", misses);
    println!("     Total requests: {}", total);
    println!("     Hit rate: {:.1}%", hit_rate);
}
