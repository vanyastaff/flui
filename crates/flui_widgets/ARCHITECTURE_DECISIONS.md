# Architecture Decisions for flui_widgets

This document explains key architectural decisions made during widget implementation.

## Container: StatelessWidget vs RenderObjectWidget

### Decision

**Container is implemented as a StatelessWidget**, not a RenderObjectWidget.

### Rationale

This follows Flutter's official design:

```
Flutter inheritance hierarchy:
Object → DiagnosticableTree → Widget → StatelessWidget → Container
```

Container is a **convenience widget** that composes other widgets, rather than directly managing layout/painting.

### Flutter's Container Implementation

In Flutter, Container's `build()` method creates a tree of widgets:

```dart
class Container extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    Widget current = child;

    if (child == null && (constraints == null || !constraints.isTight)) {
      current = LimitedBox(...);
    }

    if (alignment != null)
      current = Align(alignment: alignment, child: current);

    if (padding != null)
      current = Padding(padding: padding, child: current);

    if (decoration != null)
      current = DecoratedBox(decoration: decoration, child: current);

    if (constraints != null)
      current = ConstrainedBox(constraints: constraints, child: current);

    if (margin != null)
      current = Padding(padding: margin, child: current);

    if (transform != null)
      current = Transform(transform: transform, child: current);

    return current;
  }
}
```

### Flui Implementation

**Current state (awaiting StatelessWidget trait):**
```rust
impl Widget for Container {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        // Will be: Box::new(flui_core::ComponentElement::new(self.clone()))
        todo!("Container::create_element - requires StatelessWidget implementation")
    }
}
```

**Future implementation:**
```rust
impl StatelessWidget for Container {
    fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
        let mut current = self.child.clone();

        // Apply constraints (width, height, constraints)
        if let Some(width) = self.width || let Some(height) = self.height {
            current = Box::new(SizedBox {
                width: self.width,
                height: self.height,
                child: current,
            });
        }

        // Apply padding
        if let Some(padding) = self.padding {
            current = Box::new(Padding {
                padding,
                child: current,
            });
        }

        // Apply alignment
        if let Some(alignment) = self.alignment {
            current = Box::new(Align {
                alignment,
                child: current,
            });
        }

        // Apply decoration
        if let Some(decoration) = self.decoration {
            current = Box::new(DecoratedBox {
                decoration,
                child: current,
            });
        }

        // Apply margin
        if let Some(margin) = self.margin {
            current = Box::new(Padding {
                padding: margin,
                child: current,
            });
        }

        current
    }
}
```

## Widget Type Categories

### RenderObjectWidget

Widgets that **directly control layout and painting** via RenderObjects.

**Implemented widgets:**
- **Padding** → creates RenderPadding
- **Center** → creates RenderPositionedBox (with CENTER alignment)
- **Align** → creates RenderPositionedBox (with custom alignment)
- **SizedBox** → creates RenderConstrainedBox
- **Row** → creates RenderFlex(Horizontal) + MultiChildRenderObjectWidget
- **Column** → creates RenderFlex(Vertical) + MultiChildRenderObjectWidget

**Element type:** All use `RenderObjectElement<W: RenderObjectWidget>`

**Example:**
```rust
impl RenderObjectWidget for Center {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        Box::new(RenderPositionedBox::new(
            Alignment::CENTER,
            self.width_factor,
            self.height_factor,
        ))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        if let Some(positioned) = render_object.downcast_mut::<RenderPositionedBox>() {
            positioned.set_alignment(Alignment::CENTER);
            positioned.set_width_factor(self.width_factor);
            positioned.set_height_factor(self.height_factor);
        }
    }
}
```

### StatelessWidget

Widgets that **compose other widgets** for convenience.

**Implemented widgets:**
- **Container** (composes: Padding + Align + DecoratedBox + ConstrainedBox)

**Element type:** `ComponentElement<W: StatelessWidget>`

**Future examples:**
- AppBar (composes: Padding + Row + Align + DecoratedBox)
- Card (composes: Container + DecoratedBox + Material)
- ListTile (composes: Row + Padding + Align)

## Element Architecture in Flui vs Flutter

### Flutter

Flutter has specialized Element types:
- `RenderObjectElement`
  - `SingleChildRenderObjectElement` (for single-child widgets)
  - `MultiChildRenderObjectElement` (for multi-child widgets)
- `ComponentElement`
  - `StatelessElement` (for StatelessWidget)
  - `StatefulElement` (for StatefulWidget)

### Flui (Simplified)

Flui uses fewer Element types:
- `RenderObjectElement<W: RenderObjectWidget>` - for ALL RenderObjectWidget (both single and multi-child)
- `ComponentElement<W: StatelessWidget>` - for StatelessWidget
- `StatefulElement` - for StatefulWidget
- `InheritedElement<W: InheritedWidget>` - for InheritedWidget

**Key difference:** Single vs multi-child is determined by the **trait** (`MultiChildRenderObjectWidget`), not by Element type.

## Correction of Initial Architecture Documents

### nebula_arch_p3.txt (Outdated)

The early planning document showed:

```rust
impl Widget for Container {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(SingleChildRenderObjectElement::new(self.clone()))
    }
}

struct RenderContainer { ... }
```

**This was incorrect** because:
1. Container should be StatelessWidget, not RenderObjectWidget
2. No need for custom RenderContainer - Container composes existing RenderObjects
3. SingleChildRenderObjectElement doesn't exist in Flui (we use generic RenderObjectElement)

### Current Implementation (Correct)

Container is correctly implemented as a StatelessWidget (awaiting trait implementation):

```rust
impl Widget for Container {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        // Will create ComponentElement when StatelessWidget is ready
        todo!("Container::create_element - requires StatelessWidget implementation")
    }
}
```

## References

- [Flutter Container source](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/container.dart)
- [Flutter StatelessWidget docs](https://api.flutter.dev/flutter/widgets/StatelessWidget-class.html)
- [Flutter Container docs](https://api.flutter.dev/flutter/widgets/Container-class.html)
- [WIDGET_GUIDELINES.md](./WIDGET_GUIDELINES.md)
- [Flui RenderObjectWidget](../flui_core/src/widget.rs)
