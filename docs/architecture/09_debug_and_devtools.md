# Chapter 9: Debug & DevTools

## 📋 Overview

Debug tools помогают разработчикам понимать и оптимизировать UI. FLUI предоставляет **widget inspector**, **performance profiler**, **layer visualizer**, и **memory inspector**.

## 🔍 Widget Inspector

### Implementation

```rust
pub struct WidgetInspector {
    /// Selected element
    selected: Option<ElementId>,
    
    /// Show debug overlays
    show_bounds: bool,
    show_baselines: bool,
    show_layers: bool,
}

impl WidgetInspector {
    pub fn render(&mut self, tree: &ElementTree, ctx: &mut egui::Context) {
        egui::Window::new("Widget Inspector")
            .show(ctx, |ui| {
                // Tree view
                ui.collapsing("Widget Tree", |ui| {
                    self.render_tree(tree.root(), tree, ui);
                });
                
                // Selected widget details
                if let Some(selected) = self.selected {
                    ui.separator();
                    self.render_details(selected, tree, ui);
                }
                
                // Debug overlays
                ui.separator();
                ui.checkbox(&mut self.show_bounds, "Show Bounds");
                ui.checkbox(&mut self.show_baselines, "Show Baselines");
                ui.checkbox(&mut self.show_layers, "Show Layers");
            });
    }
    
    fn render_tree(&mut self, id: ElementId, tree: &ElementTree, ui: &mut egui::Ui) {
        let element = tree.get(id);
        let name = element.type_name();
        
        let response = ui.selectable_label(
            self.selected == Some(id),
            name,
        );
        
        if response.clicked() {
            self.selected = Some(id);
        }
        
        // Render children
        ui.indent(id, |ui| {
            for child in tree.children(id) {
                self.render_tree(child, tree, ui);
            }
        });
    }
    
    fn render_details(&self, id: ElementId, tree: &ElementTree, ui: &mut egui::Ui) {
        let element = tree.get(id);
        
        ui.heading("Details");
        ui.label(format!("Type: {}", element.type_name()));
        ui.label(format!("ID: {:?}", id));
        
        // Size
        if let Some(size) = element.size() {
            ui.label(format!("Size: {:.1} × {:.1}", size.width, size.height));
        }
        
        // Constraints
        if let Some(constraints) = element.constraints() {
            ui.label(format!("Constraints: {:?}", constraints));
        }
        
        // Properties
        ui.collapsing("Properties", |ui| {
            ui.label(format!("{:#?}", element.properties()));
        });
        
        // Debug info
        ui.collapsing("Debug Info", |ui| {
            ui.label(format!("{:#?}", element));
        });
    }
}
```

## 📊 Performance Profiler

### Implementation

```rust
pub struct PerformanceProfiler {
    /// Frame timings
    frame_times: VecDeque<FrameTiming>,
    
    /// Current frame budget (16.67ms for 60fps)
    frame_budget: Duration,
}

#[derive(Debug, Clone)]
pub struct FrameTiming {
    pub total: Duration,
    pub build: Duration,
    pub layout: Duration,
    pub paint: Duration,
    pub composite: Duration,
}

impl PerformanceProfiler {
    pub fn render(&self, ctx: &mut egui::Context) {
        egui::Window::new("Performance")
            .show(ctx, |ui| {
                // FPS counter
                let avg_frame_time = self.avg_frame_time();
                let fps = 1000.0 / avg_frame_time.as_millis() as f32;
                
                ui.heading(format!("FPS: {:.1}", fps));
                ui.label(format!("Frame time: {:.2}ms", avg_frame_time.as_millis()));
                
                // Frame time graph
                self.render_graph(ui);
                
                // Phase breakdown
                ui.separator();
                ui.heading("Phase Breakdown");
                self.render_phases(ui);
                
                // Warnings
                if avg_frame_time > self.frame_budget {
                    ui.colored_label(
                        egui::Color32::RED,
                        format!("⚠ Over budget by {:.2}ms", 
                            (avg_frame_time - self.frame_budget).as_secs_f32() * 1000.0
                        )
                    );
                }
            });
    }
    
    fn render_graph(&self, ui: &mut egui::Ui) {
        use egui::plot::*;
        
        let points: PlotPoints = self.frame_times
            .iter()
            .enumerate()
            .map(|(i, timing)| {
                [i as f64, timing.total.as_millis() as f64]
            })
            .collect();
        
        let line = Line::new(points);
        
        Plot::new("frame_times")
            .height(200.0)
            .show(ui, |plot_ui| {
                plot_ui.line(line);
                
                // Budget line (16.67ms for 60fps)
                plot_ui.hline(HLine::new(16.67).color(Color32::RED));
            });
    }
    
    fn render_phases(&self, ui: &mut egui::Ui) {
        if let Some(last_frame) = self.frame_times.back() {
            ui.label(format!("Build:     {:.2}ms", last_frame.build.as_secs_f32() * 1000.0));
            ui.label(format!("Layout:    {:.2}ms", last_frame.layout.as_secs_f32() * 1000.0));
            ui.label(format!("Paint:     {:.2}ms", last_frame.paint.as_secs_f32() * 1000.0));
            ui.label(format!("Composite: {:.2}ms", last_frame.composite.as_secs_f32() * 1000.0));
        }
    }
    
    fn avg_frame_time(&self) -> Duration {
        let sum: Duration = self.frame_times.iter().map(|t| t.total).sum();
        sum / self.frame_times.len() as u32
    }
}
```

## 🎨 Layer Visualizer

```rust
pub struct LayerVisualizer {
    /// Show layer bounds
    show_bounds: bool,
    
    /// Show layer types
    show_types: bool,
    
    /// Highlight selected layer
    selected_layer: Option<LayerId>,
}

impl LayerVisualizer {
    pub fn render_overlay(&self, layer: &BoxedLayer, canvas: &mut Canvas) {
        if self.show_bounds {
            self.draw_bounds(layer, canvas);
        }
        
        if self.show_types {
            self.draw_type_label(layer, canvas);
        }
        
        // Recurse to children
        layer.visit_children(&mut |child| {
            self.render_overlay(child, canvas);
        });
    }
    
    fn draw_bounds(&self, layer: &BoxedLayer, canvas: &mut Canvas) {
        let bounds = layer.bounds();
        
        let paint = Paint::new()
            .color(Color::from_rgba(255, 0, 0, 128))
            .style(PaintStyle::Stroke)
            .stroke_width(2.0);
        
        canvas.draw_rect(bounds, &paint);
    }
    
    fn draw_type_label(&self, layer: &BoxedLayer, canvas: &mut Canvas) {
        let bounds = layer.bounds();
        let type_name = layer.layer_type();
        
        // Draw label at top-left of bounds
        let label_pos = bounds.top_left() + Offset::new(2.0, 2.0);
        
        // TODO: Draw text label
    }
}
```

## 💾 Memory Inspector

```rust
pub struct MemoryInspector {
    /// Widget count by type
    widget_counts: HashMap<String, usize>,
    
    /// Total memory usage
    total_memory: usize,
}

impl MemoryInspector {
    pub fn analyze(&mut self, tree: &ElementTree) {
        self.widget_counts.clear();
        self.total_memory = 0;
        
        self.analyze_element(tree.root(), tree);
    }
    
    fn analyze_element(&mut self, id: ElementId, tree: &ElementTree) {
        let element = tree.get(id);
        
        // Count by type
        let type_name = element.type_name();
        *self.widget_counts.entry(type_name.to_string()).or_insert(0) += 1;
        
        // Estimate memory (rough approximation)
        self.total_memory += std::mem::size_of_val(element);
        
        // Recurse to children
        for child in tree.children(id) {
            self.analyze_element(child, tree);
        }
    }
    
    pub fn render(&self, ctx: &mut egui::Context) {
        egui::Window::new("Memory Inspector")
            .show(ctx, |ui| {
                ui.heading(format!("Total: {:.2} MB", self.total_memory as f32 / 1_000_000.0));
                
                ui.separator();
                ui.heading("Widget Counts");
                
                // Sort by count (descending)
                let mut counts: Vec<_> = self.widget_counts.iter().collect();
                counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
                
                for (type_name, count) in counts {
                    ui.label(format!("{}: {}", type_name, count));
                }
            });
    }
}
```

## 🐛 Debug Assertions

```rust
#[cfg(debug_assertions)]
pub fn check_layout_invariants(element: &Element, size: Size, constraints: BoxConstraints) {
    // Check 1: Size respects constraints
    assert!(
        constraints.contains(size),
        "Element {:?} returned size {:?} which violates constraints {:?}",
        element.type_name(),
        size,
        constraints
    );
    
    // Check 2: Size is finite
    assert!(
        size.width.is_finite() && size.height.is_finite(),
        "Element {:?} returned non-finite size {:?}",
        element.type_name(),
        size
    );
    
    // Check 3: Size is non-negative
    assert!(
        size.width >= 0.0 && size.height >= 0.0,
        "Element {:?} returned negative size {:?}",
        element.type_name(),
        size
    );
}
```

## 🔧 Debug Modes

```rust
pub struct DebugFlags {
    /// Enable layout debug overlay
    pub debug_layout: bool,
    
    /// Enable paint debug overlay
    pub debug_paint: bool,
    
    /// Enable performance metrics
    pub debug_performance: bool,
    
    /// Enable memory tracking
    pub debug_memory: bool,
    
    /// Enable verbose logging
    pub verbose: bool,
}

impl DebugFlags {
    pub fn from_env() -> Self {
        Self {
            debug_layout: std::env::var("FLUI_DEBUG_LAYOUT").is_ok(),
            debug_paint: std::env::var("FLUI_DEBUG_PAINT").is_ok(),
            debug_performance: std::env::var("FLUI_DEBUG_PERF").is_ok(),
            debug_memory: std::env::var("FLUI_DEBUG_MEMORY").is_ok(),
            verbose: std::env::var("FLUI_VERBOSE").is_ok(),
        }
    }
}

// Usage:
// FLUI_DEBUG_LAYOUT=1 cargo run
```

## 🔗 Cross-References

- **Previous:** [Chapter 8: Frame Scheduler](08_frame_scheduler.md)
- **Next:** [Chapter 10: Future Extensions](10_future_extensions.md)
- **Related:** [Appendix C: Performance Guide](appendix_c_performance.md)

---

**Key Takeaway:** FLUI's debug tools provide deep insights into widget hierarchy, performance bottlenecks, and memory usage for efficient development and optimization!
