# Профилирование и Бенчмарки

> Руководство по профилированию и измерению производительности в Flui

## 🎯 Обзор

Flui предоставляет встроенную инфраструктуру для профилирования и бенчмарков, которая помогает:
- Измерять производительность кода
- Находить узкие места
- Проверять эффект от оптимизаций
- Мониторить производительность в реальном времени

---

## 📊 Профилирование

### Включение профилирования

Добавьте feature при запуске:

```bash
# Puffin (in-app профилирование)
cargo run --features profiling

# Tracy (внешний профайлер)
cargo run --features tracy

# Оба вместе
cargo run --features full-profiling
```

### Использование макросов

#### 1. Профилирование функции

```rust
use flui_core::profiling::profile_function;

fn expensive_function() {
    profile_function!(); // Автоматически использует имя функции

    // ... ваш код ...
}
```

#### 2. Профилирование scope

```rust
use flui_core::profiling::profile_scope;

fn complex_function() {
    profile_scope!("initialization");
    initialize();

    profile_scope!("computation");
    compute();

    profile_scope!("cleanup");
    cleanup();
}
```

#### 3. Профилирование выражения

```rust
use flui_core::profiling::profile_expr;

let result = profile_expr!("expensive_calc", {
    very_expensive_calculation()
});
```

### Инициализация

В вашем `main.rs`:

```rust
fn main() {
    // Инициализация профилирования
    flui_core::profiling::init();

    // Запуск HTTP сервера (для puffin)
    #[cfg(feature = "profiling")]
    flui_core::profiling::start_server();

    println!("Puffin server: http://localhost:8585");

    // Основной цикл приложения
    loop {
        render_frame();

        // Отметить конец кадра для профилирования
        flui_core::profiling::finish_frame();
    }
}
```

### Просмотр результатов

#### Puffin Viewer

1. Запустите приложение с `--features profiling`
2. Откройте http://localhost:8585 в браузере
3. Или используйте `puffin_viewer`:

```bash
cargo install puffin_viewer
puffin_viewer
# Подключитесь к localhost:8585
```

#### Tracy

1. Скачайте Tracy profiler
2. Запустите приложение с `--features tracy`
3. Подключите Tracy к процессу

---

## 🏃 Бенчмарки

### Запуск бенчмарков

```bash
# Все бенчмарки
cargo bench

# Конкретный бенчмарк
cargo bench --bench layout_cache

# С фильтром
cargo bench layout_cache_hit
```

### Доступные бенчмарки

#### Layout Cache Benchmarks

```bash
cargo bench --bench layout_cache
```

Измеряет:
- **layout_no_cache** - Baseline без кеширования
- **layout_cache_hit** - Попадание в кеш (должно быть ~100x быстрее)
- **layout_cache_miss** - Промах кеша
- **layout_cache_scaling** - Масштабирование с 10-10000 записей
- **layout_cache_invalidate** - Производительность инвалидации

#### String Interning Benchmarks

Измеряет:
- **string_intern** - Интернирование новой строки
- **string_intern_cached** - Интернирование существующей строки
- **string_resolve** - Получение строки по handle
- **string_comparison** - Сравнение intern-строк (O(1))

### Создание своих бенчмарков

Создайте файл `benches/my_benchmark.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_core::*;

fn bench_my_function(c: &mut Criterion) {
    c.bench_function("my_function", |b| {
        b.iter(|| {
            black_box(my_function(black_box(input)))
        });
    });
}

criterion_group!(benches, bench_my_function);
criterion_main!(benches);
```

---

## 📈 Интерпретация результатов

### Пример вывода бенчмарка

```
layout_cache_hit        time:   [12.345 ns 12.567 ns 12.789 ns]
layout_no_cache         time:   [1.2345 μs 1.2567 μs 1.2789 μs]
```

**Анализ:**
- Cache hit: ~12.5 ns
- No cache: ~1.25 μs
- **Speedup: ~100x** ✅

### Что означают метрики

- **time**: Среднее время выполнения
- **[min avg max]**: Диапазон измерений
- **change**: Изменение относительно предыдущего запуска
- **R²**: Качество измерения (ближе к 1.0 = лучше)

### Целевые показатели

| Операция | Целевое время | Статус |
|----------|---------------|--------|
| Layout (cached) | < 100ns | ✅ Достигнуто |
| Layout (no cache) | < 10μs | ✅ Достигнуто |
| String intern (cached) | < 20ns | ✅ Достигнуто |
| String comparison | < 5ns | ✅ Достигнуто |
| Frame time | < 16ms (60 FPS) | 🎯 Цель |

---

## 🔍 Примеры использования

### Пример 1: Профилирование layout

```rust
use flui_core::profiling::profile_function;
use flui_core::cache::{get_layout_cache, LayoutCacheKey, LayoutResult};

impl RenderBox for MyRenderBox {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        profile_function!();

        let key = LayoutCacheKey::new(self.id, constraints);
        let cache = get_layout_cache();

        let result = cache.get_or_compute(key, || {
            profile_scope!("expensive_layout");
            LayoutResult::new(self.compute_intrinsic_size(constraints))
        });

        result.size
    }
}
```

### Пример 2: Профилирование widget tree build

```rust
use flui_core::profiling::{profile_function, profile_scope};

fn build_widget_tree() {
    profile_function!();

    profile_scope!("create_root");
    let root = create_root_widget();

    profile_scope!("build_children");
    for child in children {
        profile_scope!("build_child");
        build_child(child);
    }

    profile_scope!("layout");
    perform_layout();

    profile_scope!("paint");
    paint();
}
```

### Пример 3: Frame профилирование

```rust
fn main_loop() {
    loop {
        profile_scope!("frame");

        {
            profile_scope!("update");
            update_state();
        }

        {
            profile_scope!("build");
            build_ui();
        }

        {
            profile_scope!("layout");
            layout();
        }

        {
            profile_scope!("paint");
            paint();
        }

        flui_core::profiling::finish_frame();
    }
}
```

---

## 🎨 Визуализация результатов

### Puffin Timeline View

```
Frame 0  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 16.2ms
  └─ update ━━━━ 2.1ms
  └─ build ━━━━━━━━━━ 8.5ms
      └─ layout_cache_hit ┅ 0.012μs (cached!)
  └─ layout ━━━ 3.2ms
  └─ paint ━━ 2.4ms
```

### Flame Graph (Tracy)

Показывает где проводится больше всего времени в виде flame graph.

---

## 📚 Ресурсы

### Инструменты

- [Puffin](https://github.com/EmbarkStudios/puffin) - In-app профайлер для Rust
- [Tracy](https://github.com/wolfpld/tracy) - Мощный frame профайлер
- [Criterion](https://github.com/bheisler/criterion.rs) - Статистические бенчмарки

### Документация

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Flamegraph Guide](https://www.brendangregg.com/flamegraphs.html)

---

## ✅ Чеклист оптимизации

При оптимизации производительности:

- [ ] Измерьте до оптимизации (baseline)
- [ ] Добавьте profile_function!() в подозрительные функции
- [ ] Запустите профайлер и найдите hotspots
- [ ] Оптимизируйте самые медленные части
- [ ] Добавьте бенчмарки для критических путей
- [ ] Измерьте после оптимизации
- [ ] Проверьте, что улучшение > 10%
- [ ] Коммитьте бенчмарки вместе с кодом

---

## 🚀 Быстрый старт

```bash
# 1. Запустите demo с профилированием
cargo run --example profiling_demo --features profiling

# 2. Откройте http://localhost:8585

# 3. Запустите бенчмарки
cargo bench --bench layout_cache

# 4. Смотрите результаты в target/criterion/
```

---

**Версия:** 1.0
**Дата:** 2025-01-19
**Статус:** ✅ ГОТОВО К ИСПОЛЬЗОВАНИЮ
