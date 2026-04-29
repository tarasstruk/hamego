# Plan: WebAssembly Module for Browser SVG Rendering

## Goal

Add a WASM module that generates SVG images in the browser from HPGL input using `web_sys` and `wasm_bindgen`, reusing the core parsing logic.

## Steps

### 1. Restructure into a Cargo workspace

Convert the root `Cargo.toml` into a workspace with two members:
- `hamego` — existing CLI crate (keeps `vsvg`, `clap`, `serialport` deps)
- `hamego-wasm` — new WASM crate (`wasm-bindgen`, `web-sys`)

This avoids feature-flag complexity — WASM-incompatible deps never touch the WASM target.

### 2. Extract core parsing module

Create a shared `hamego-core` crate (or `src/core.rs` module) containing:
- `Config` struct (without `clap` derives — plain fields with `Default`)
- Color constants as simple RGB hex strings (`const COLORS: [&str; 5]`)
- `extract_points()`, `read_points()` — pure coordinate math
- New `parse_commands(hpgl: &str, config: &Config) -> Vec<DrawCommand>` returning a platform-agnostic command list:

```rust
enum DrawCommand {
    SelectPen(usize),
    PenUp(f64, f64),
    PenDown(Vec<(f64, f64)>),
}
```

Zero platform dependencies — only `itertools`.

### 3. Build SVG string generator

Implement in the shared core:

```rust
fn generate_svg_string(hpgl: &str, config: &Config) -> String
```

Iterates `DrawCommand`s and writes SVG markup (`<svg>`, `<polyline>` elements with stroke colors) using `format!`/`write!`. No `vsvg` dependency needed.

### 4. Create `hamego-wasm` crate

New crate at `hamego-wasm/` with dependencies:
- `wasm-bindgen`
- `web-sys` with features: `Document`, `Element`, `HtmlElement`, `Window`, `SvgElement`
- `hamego-core`

Exported `#[wasm_bindgen]` API:

```rust
/// Returns SVG string from HPGL input
#[wasm_bindgen]
pub fn render_hpgl(hpgl: &str, scale: f64, width: f64, height: f64, stroke_width: f64) -> String

/// Injects SVG directly into a DOM container
#[wasm_bindgen]
pub fn render_hpgl_to_dom(hpgl: &str, scale: f64, width: f64, height: f64, stroke_width: f64, container_id: &str)
```

### 5. Build configuration and browser example

- Build with: `wasm-pack build hamego-wasm --target web`
- Add `hamego-wasm/www/index.html` — minimal page that:
  - Loads the WASM module
  - Provides a textarea for HPGL input + config sliders (scale, width, height, stroke_width)
  - Renders SVG on button click
- Optional: add a `Makefile` or npm script for the build step

### 6. Migrate existing `hamego` CLI crate

Update `src/lib.rs` `elaborate()` to use `parse_commands()` internally, then convert `DrawCommand`s to `vsvg::Path` objects. Preserves current CLI behavior while sharing parsing logic.


## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Workspace vs feature flags | Workspace | Cleaner — `vsvg`/`clap`/`serialport` never attempt WASM compilation |
| SVG generation in WASM | String building | `vsvg` likely won't compile to `wasm32-unknown-unknown`; `format!`-based SVG is simple and sufficient |
| Color mapping | Hex strings in core | `vsvg::Color` unavailable in WASM; hex strings (`#8B0000`, `#008000`, etc.) work for both SVG string output and DOM attributes |
| DOM injection | Optional | `render_hpgl()` returns string (flexible); `render_hpgl_to_dom()` is convenience for direct DOM use |

