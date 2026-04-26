# AGENTS.md

## Project Overview

Hamego is a Rust toolkit for interfacing with the Hameg HM1507 oscilloscope. It converts HPGL (Hewlett-Packard Graphics Language) plotter output from the oscilloscope to SVG. It parses HPGL commands (`SP`, `PU`, `PD`) from `.hpgl` sample files and renders them as vector paths using the `vsvg` crate.

## Architecture

- **`src/main.rs`** — HPGL parser and SVG generator. Parses semicolon-delimited HPGL commands, transforms plotter coordinates to SVG coordinates (Y-axis flip, scaling via `SCALE`), and writes `test.svg`.
- **`src/connection.rs`** — Ignore this file.

## Key Conventions

- **Coordinate system**: HPGL uses bottom-left origin (max 6540×4400 plotter units). Conversion flips Y (`4400 - y`) and applies `SCALE` (0.25) to map to SVG pixels.
- **HPGL command parsing**: Commands are split by `;`. `SP<n>` selects pen/color, `PU<coords>` is pen-up (move), `PD<coords>` is pen-down (draw). Coordinates are comma-separated `x,y` pairs.
- **`StartingPoint`**: A one-shot iterator that captures the last `PU` position and chains it as the first point of the next `PD` polyline. This bridges pen-up moves to pen-down draws.
- **Color mapping**: `COLORS` array maps pen numbers (from `SP`) to `vsvg::Color` constants (index 0–4).

## Build & Run

```sh
cargo build
cargo run -- samples/test.hpgl              # Writes samples/test.svg
cargo run -- samples/test.hpgl -o test.svg  # Custom output path
```

No tests exist yet. Sample HPGL files are in `samples/`.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `vsvg` (0.4.0) | SVG document model, path/layer construction, SVG file output |
| `serialport` (4.5.0) | Serial port communication with physical plotters |
| `itertools` (0.13.0) | `.tuples()` for pairing consecutive x,y values from flat coordinate lists |

## Rust Edition

Uses **Rust 2024 edition** (`edition = "2024"` in Cargo.toml). Ensure your toolchain supports this.

## Adding New HPGL Commands

Follow the pattern in `main.rs` line 64–89: match on the command prefix string (e.g., `cmd.starts_with("XX")`), parse arguments from `&cmd[2..]` using `read_points()` or `extract_points()`, and update document/layer state accordingly.

