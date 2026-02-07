# GRD Code Style and Conventions

## Module Layout
- **No `mod.rs` files** - Use modern Edition 2024 pattern
- For module `x`, use `src/x.rs` and `src/x/` for children
- Each module is a separate file in `src/`

## Code Style
- **Edition**: Rust 2024
- **Error Handling**: Use `anyhow::{Context, Result}` for error propagation
- **CLI**: Use `clap` with derive macros
- **Serialization**: Use `serde` derive macros

## Naming Conventions
- Structs: `PascalCase` (e.g., `Args`, `Release`, `Asset`)
- Functions: `snake_case` (e.g., `download_asset`, `extract_zip`)
- Constants: `SCREAMING_SNAKE_CASE` (not currently used)
- Modules: `snake_case` files

## Documentation
- No explicit documentation comments currently present
- README.md is the primary user documentation

## Testing Strategy
- **NO tests currently exist** in the codebase
- AGENTS.md specifies: Place integration tests in `src/` using `#[cfg(test)]`
- Do NOT create top-level `tests/` directory

## Error Handling Patterns
```rust
// Use .context() for adding context to errors
operation().context("Failed to do something")?;

// Use .with_context() for closures
operation().with_context(|| format!("Failed for {}", value))?;
```

## Common Patterns
- Use ` anyhow::anyhow!()` for custom errors
- Use `io::{self, Read, Write}` for I/O operations
- Use trait objects (`Box<dyn Trait>`) for dynamic dispatch
