# nebula-ui Examples

This directory contains examples demonstrating various features of the nebula-ui crate.

## Running Examples

### Text Widget Demo

Demonstrates the Flutter-style Text widget with various styling options:

```bash
cargo run --example text_demo -p nebula-ui
```

Features demonstrated:
- Simple text display
- Text styles (headlines, body, code, etc.)
- Text colors
- Text alignment (left, center, right)
- Text wrapping and overflow
- Max lines with ellipsis
- Text scaling
- Bold and italic text
- Multiple text styles

## Available Examples

| Example | Description | Command |
|---------|-------------|---------|
| `text_demo` | Comprehensive Text widget showcase | `cargo run --example text_demo -p nebula-ui` |

## Building Examples

To build all examples without running:

```bash
cargo build --examples -p nebula-ui
```

To build a specific example:

```bash
cargo build --example text_demo -p nebula-ui
```
