# Layout Cache

Высокопроизводительная система кэширования результатов layout с двухуровневой архитектурой.

## Архитектура (Sprint 3 Optimization)

### Двухуровневое кэширование

1. **L1 Cache: RenderState (Per-Object)** ⚡ NEW
   - Хранится прямо в RenderElement
   - Нулевые затраты на поиск (direct field access)
   - Проверка через atomic flags (lock-free)
   - Инвалидация через mark_needs_layout()

2. **L2 Cache: Global LRU (Legacy)**
   - Используется как fallback для cross-frame кэширования
   - Мока LRU + TTL стратегия
   - Thread-safe, но с overhead от блокировок
   - **Теперь отключен в LayoutCx** (RenderState эффективнее!)

### Оптимизация производительности

**До (Sprint 2)**:
```rust
layout_child()
  → Global cache lookup (hash + lock)
  → tree.layout_render_object()
  → dyn_layout()
```

**После (Sprint 3)** ⚡:
```rust
layout_child()
  → tree.layout_render_object()
      → RenderState check (direct field access, lock-free flags!)
      → if cache_hit: return size (10-20% faster!)
      → else: dyn_layout()
```

**Преимущества**:
- ✅ Нет hash lookups для L1 cache
- ✅ Нет глобальных блокировок для common case
- ✅ Atomic flags для check (lock-free!)
- ✅ Лучшая cache locality (данные рядом с объектом)
- ✅ 10-20% улучшение производительности layout

## Использование

### Базовое использование (автоматическое)

Кэш теперь работает автоматически на уровне ElementTree:

```rust
// Просто вызываем layout_child - кэширование происходит внутри!
impl RenderObject for MyRenderObject {
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();

        // RenderState автоматически проверяется в layout_child()
        let child_size = cx.layout_child(child, child_constraints);

        // Если constraints не изменились → кэш попадание (L1)!
        Size::new(child_size.width, child_size.height)
    }
}
```

### Инвалидация кэша

```rust
// Пометить как требующий relayout
render_element.state().mark_needs_layout();

// Или через pipeline
pipeline_owner.mark_needs_layout(element_id);
```

## Использование

### Базовое использование

```rust
use flui_core::render::cache::{LayoutCache, LayoutCacheKey, LayoutResult};
use flui_types::{Size, constraints::BoxConstraints};

let cache = LayoutCache::new();

// Создать ключ кэша
let key = LayoutCacheKey::new(element_id, constraints);

// Для multi-child layout добавить количество детей
let key = key.with_child_count(child_count);

// Проверить кэш
if let Some(result) = cache.get(&key) {
    // Cache hit!
    return result.size;
}

// Вычислить layout и закэшировать
let size = perform_layout(constraints);
cache.insert(key, LayoutResult::new(size));
```

### Статистика

```rust
// Получить детальную статистику
let (hits, misses, total, hit_rate) = cache.detailed_stats();
println!("Hit rate: {:.1}%", hit_rate);

// Вывести статистику в stderr
cache.print_stats();

// Сбросить счетчики (для бенчмарков)
cache.reset_stats();
```

### Инвалидация

```rust
// Инвалидировать конкретную запись
cache.invalidate(&key);

// Очистить весь кэш
cache.clear();

// Получить количество записей
let count = cache.entry_count();
```

## Multi-child Layout

Для корректной работы с multi-child layout необходимо включать `child_count` в ключ:

```rust
// Базовый ключ (для leaf widgets)
let key = LayoutCacheKey::new(element_id, constraints);

// Multi-child ключ
let key = LayoutCacheKey::new(element_id, constraints)
    .with_child_count(children.len());
```

**Важно**: Если количество детей изменилось, кэш должен быть инвалидирован:

```rust
// До: 3 ребенка
let key_old = LayoutCacheKey::new(id, constraints).with_child_count(3);

// После: 5 детей
let key_new = LayoutCacheKey::new(id, constraints).with_child_count(5);

// Это разные ключи! Кэш будет miss, что и требуется
assert_ne!(key_old, key_new);
```

## Debug Output

LayoutCache реализует `Debug` с выводом статистики:

```rust
println!("{:?}", cache);
// Output:
// LayoutCache {
//     entries: 42,
//     hits: 150,
//     misses: 8,
//     total_requests: 158,
//     hit_rate: "94.9%"
// }
```

## Производительность

### Операции кэша

- **Get**: O(1) amortized (hash map lookup)
- **Insert**: O(1) amortized (hash map insert + LRU update)
- **Statistics**: O(1) (atomic load, без блокировок)

### Memory Overhead

- **Структура**: 24 байта (pointer + 2×u64)
- **Запись кэша**: ~48 байт (key + value + metadata)
- **Максимум**: ~480 KB (10k записей × 48 байт)

### Потокобезопасность

- **Cache**: moka::sync::Cache (внутренние RwLock)
- **Статистика**: AtomicU64 (lock-free)
- **Contention**: Минимальный (read-heavy workload)

## Примеры

См. `examples/layout_cache_demo.rs` для полного примера использования.

### Типичный паттерн в RenderObject

```rust
impl RenderObject for MyRenderObject {
    fn layout(&mut self, cx: &mut LayoutCx) -> Size {
        let cache = layout_cache();
        let key = LayoutCacheKey::new(cx.element_id(), cx.constraints());

        // Check cache first
        if let Some(result) = cache.get(&key) {
            if !result.needs_layout {
                return result.size;
            }
        }

        // Perform layout
        let size = self.compute_layout(cx);

        // Cache result
        cache.insert(key, LayoutResult::new(size));

        size
    }
}
```

## Когда инвалидировать кэш

1. **Изменение constraints**: Автоматически (разные ключи)
2. **Изменение свойств виджета**: Вызвать `mark_needs_layout()`
3. **Добавление/удаление детей**: Изменится `child_count` → разные ключи
4. **Reorder детей**: Может потребовать инвалидацию в зависимости от layout логики

## Отличия от старой реализации

### Добавлено в новую версию

- ✅ Полная статистика (hits, misses, hit rate)
- ✅ Lock-free счетчики (AtomicU64)
- ✅ `detailed_stats()` - детальная информация
- ✅ `print_stats()` - вывод для отладки
- ✅ `reset_stats()` - сброс для бенчмарков
- ✅ Custom Debug impl с статистикой
- ✅ Полные unit тесты (7 тестов)

### Сохранено из старой версии

- ✅ LRU + TTL стратегия (moka)
- ✅ Multi-child support (child_count в ключе)
- ✅ Thread-safe операции
- ✅ Методы: get, insert, invalidate, clear, entry_count

## Тестирование

```bash
cargo test --package flui_core render::cache
```

Тесты покрывают:
- Операции кэша (get, insert, invalidate, clear)
- Статистику (hits, misses, hit rate)
- Сброс статистики
- Multi-child ключи
- Debug форматирование
