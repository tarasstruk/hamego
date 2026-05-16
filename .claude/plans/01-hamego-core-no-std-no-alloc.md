# Plan: hamego-core `no_std` without `alloc`

## Goal

Remove all `alloc` dependencies from `hamego-core` so it compiles on targets without a heap allocator (e.g. embedded).

## Current allocating APIs

| Symbol | Alloc usage |
|--------|-------------|
| `DrawCommand::PenDown(Vec<(f64, f64)>)` | `Vec` |
| `parse_commands() -> Vec<DrawCommand>` | `Vec` return |
| `generate_svg_string() -> String` | `String`, `format!` |
| `itertools::Itertools::tuples()` | crate uses `alloc` feature |
| `Vec<_>.join(" ")` in SVG builder | `Vec`, `String` |

## Steps

### 1. Replace `parse_commands()` with a callback trait

Instead of collecting into `Vec<DrawCommand>`, define a handler trait:

```rust
pub trait CommandHandler {
    fn select_pen(&mut self, pen: usize);
    fn pen_up(&mut self, x: f64, y: f64);
    fn pen_down_begin(&mut self);
    fn pen_down_point(&mut self, x: f64, y: f64);
    fn pen_down_end(&mut self);
}

pub fn parse_hpgl(hpgl: &str, config: &Config, handler: &mut impl CommandHandler)
```

The parser streams events to the handler as it encounters commands — no intermediate storage needed.

### 2. Replace `generate_svg_string()` with `generate_svg(writer: &mut impl core::fmt::Write)`

`core::fmt::Write` is available without `alloc`. The caller provides the write target:
- CLI: passes a `String` (which implements `fmt::Write`)
- WASM: passes a `String`
- Embedded: passes a `heapless::String<N>` or a fixed-size buffer wrapper

Use `write!()` macro instead of `format!()` + `push_str()`.

### 3. Remove `itertools` dependency

Replace `.tuples()` with a manual pair iterator:

```rust
fn pair_iter(input: &str) -> impl Iterator<Item = (f64, f64)> + '_ {
    let mut iter = input.split(',').map(|s| f64_from_str(s.trim()));
    core::iter::from_fn(move || {
        let x = iter.next()?;
        let y = iter.next()?;
        Some((x, y))
    })
}
```

### 4. Replace `f64::from_str` with a minimal `core`-compatible parser

`core::str::FromStr` for `f64` is available in `core` (since Rust 1.0), so no change needed here. However, the `unwrap()` call should remain or be replaced with a returned error — no alloc impact either way.

### 5. Remove `DrawCommand` enum (optional)

With the callback approach, `DrawCommand` becomes unnecessary as a public type. It can be kept as internal documentation or removed entirely to simplify the API.

### 6. Implement `SvgWriter` as a `CommandHandler`

```rust
pub struct SvgWriter<'a, W: core::fmt::Write> {
    writer: &'a mut W,
    current_color: &'static str,
    stroke_width: f64,
}

impl<W: core::fmt::Write> CommandHandler for SvgWriter<'_, W> {
    fn select_pen(&mut self, pen: usize) { ... }
    fn pen_down_begin(&mut self) { /* write <polyline points=" */ }
    fn pen_down_point(&mut self, x: f64, y: f64) { /* write x,y */ }
    fn pen_down_end(&mut self) { /* close polyline tag */ }
    fn pen_up(&mut self, _x: f64, _y: f64) {}
}
```

### 7. Update `Cargo.toml`

```toml
[dependencies]
# no dependencies
```

Zero external dependencies.

## Summary of API changes

| Before (alloc) | After (no_std, no alloc) |
|---|---|
| `parse_commands(&str, &Config) -> Vec<DrawCommand>` | `parse_hpgl(&str, &Config, &mut impl CommandHandler)` |
| `generate_svg_string(&str, &Config) -> String` | `generate_svg(&str, &Config, &mut impl core::fmt::Write)` |
| `DrawCommand` enum with `Vec` variant | `CommandHandler` trait (streaming) |
| `itertools` dependency | Manual pair iterator |
| `format!()` / `String` concatenation | `write!()` to `impl fmt::Write` |

## Trade-offs

- **Pro**: Zero dependencies, works on embedded targets without allocator
- **Con**: Callback API is more complex for callers than `Vec<DrawCommand>`
- **Mitigation**: Provide `generate_svg()` as the simple high-level API; most callers never need `CommandHandler` directly

