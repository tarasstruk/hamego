# Plan: Async HPGL Parser (`parse_hpgl_async` + `AsyncCommandHandler`)

## Goal

Add an async variant of the HPGL parser that reads from an async byte stream instead of a `&str` slice. This supports real-time parsing from serial ports, network sockets, or any `AsyncRead` source.

## Key Requirements

1. **Input**: async reader (`impl AsyncRead`) instead of `&str`
2. **Termination**: byte `0x0A` (LF) signals end of a complete HPGL transmission → triggers `complete()` event
3. **New trait**: `AsyncCommandHandler` with async methods including `complete()`
4. **Location**: separate module inside `hamego-core` crate

## Design

### `AsyncCommandHandler` trait

```rust
pub trait AsyncCommandHandler {
    async fn select_pen(&mut self, pen: usize);
    async fn pen_up(&mut self, x: f64, y: f64);
    async fn pen_down_begin(&mut self);
    async fn pen_down_point(&mut self, x: f64, y: f64);
    async fn pen_down_end(&mut self);
    /// Called when 0x0A is received, signalling end of HPGL transmission.
    async fn complete(&mut self);
}
```

Uses async fn in traits (stabilized in Rust 1.75+, available in edition 2024).

### `parse_hpgl_async` function

```rust
pub async fn parse_hpgl_async<R, H>(
    reader: R,
    config: &Config,
    handler: &mut H,
) where
    R: AsyncRead + Unpin,
    H: AsyncCommandHandler,
```

### Module structure

New file `hamego-core/src/async_parser.rs`, re-exported from `lib.rs`:

```
hamego-core/src/
  lib.rs           — existing sync API + `pub mod async_parser;`
  async_parser.rs  — AsyncCommandHandler, parse_hpgl_async
```

## Steps

### 1. Add `embedded-io-async` dependency

`hamego-core` is `#![no_std]`. Use `embedded-io-async::Read` trait as the async reader abstraction — it is `no_std` compatible, unlike `tokio::io::AsyncRead`.

```toml
[dependencies]
embedded-io-async = "0.6"
```

Alternative: define a minimal `trait AsyncRead` inside the crate to avoid external deps. Trade-off: less ecosystem interop but zero dependencies.

### 2. Create `hamego-core/src/async_parser.rs`

Contents:

- `AsyncCommandHandler` trait (async fn methods + `complete()`)
- `parse_hpgl_async()`:
  - Reads bytes one-at-a-time or into a small stack buffer from the async reader
  - Accumulates a command buffer (fixed-size `[u8; N]` on stack — no alloc)
  - On `;` delimiter: parse the accumulated command, dispatch to handler (reuse `read_points` / `pair_iter` from parent module)
  - On `0x0A`: call `handler.complete()` and return
  - On EOF (read returns 0 bytes): return without calling `complete()`

### 3. Internal command buffer

Since `no_std` + no `alloc`, use a fixed-size stack buffer:

```rust
const CMD_BUF_SIZE: usize = 256; // max single HPGL command length
let mut buf = [0u8; CMD_BUF_SIZE];
let mut len = 0;
```

HPGL commands are short (typically <100 bytes), so 256 bytes is generous.

### 4. Command dispatch (reuse sync logic)

Extract the command-matching logic from `parse_hpgl` into a shared helper:

```rust
// In lib.rs — shared between sync and async
pub(crate) fn dispatch_command(
    cmd: &str,
    config: &Config,
    current: &mut Option<(f64, f64)>,
) -> ParsedCommand { ... }
```

Or duplicate the small match block in async_parser — simpler, avoids refactoring sync path.

Preferred: duplicate, since the match block is ~30 lines and async handler calls need `.await`.

### 5. Re-export from `lib.rs`

```rust
pub mod async_parser;
pub use async_parser::{AsyncCommandHandler, parse_hpgl_async};
```

### 6. Feature gate (optional)

Gate the async module behind a feature flag to keep the default build dependency-free:

```toml
[features]
default = []
async = ["embedded-io-async"]
```

```rust
#[cfg(feature = "async")]
pub mod async_parser;
```

## Tests

All tests live in `hamego-core/tests/async_parser_tests.rs`. Use a mock `AsyncRead` implementation backed by `&[u8]` slices. Use `futures-lite::future::block_on` (or a minimal executor) to drive async tests.

### Test 1: `async_parses_single_command`

Feed `b"SP1;"` + `0x0A`. Assert `select_pen(1)` and `complete()` are called.

### Test 2: `async_parses_pen_up_pen_down_sequence`

Feed `b"PU100,200;PD100,200,300,400;"` + `0x0A`. Assert:
- `pen_up()` called with correct scaled coords
- `pen_down_begin` → `pen_down_point` (3 points: PU carry + 2 PD points) → `pen_down_end`
- `complete()` called

### Test 3: `async_complete_fires_on_0x0A`

Feed `b"SP1;\x0A"`. Assert `complete()` is called exactly once.

### Test 4: `async_no_complete_on_eof`

Feed `b"SP1;"` with **no** `0x0A` (reader returns EOF). Assert `select_pen(1)` fires but `complete()` is **not** called.

### Test 5: `async_ignores_data_after_0x0A`

Feed `b"SP1;\x0ASP2;"`. Assert only `select_pen(1)` + `complete()` — `SP2` is never dispatched.

### Test 6: `async_empty_pu_is_skipped`

Feed `b"PU;"` + `0x0A`. Assert only `complete()` is called (no `pen_up` event).

### Test 7: `async_parity_with_sync_parser`

Read `samples/test5.hpgl` into bytes, wrap in mock async reader. Compare event counts (path count, pen selections, color distribution) against the existing sync `parse_hpgl` results from `test5_integration.rs`. Both must produce identical output.

### Test 8: `async_buffer_overflow_handled`

Feed a single command longer than `CMD_BUF_SIZE` (256 bytes). Assert the parser does not panic — either truncates gracefully or returns an error.

### Mock infrastructure

```rust
struct RecordingHandler {
    events: Vec<Event>,  // alloc OK in tests
}

enum Event {
    SelectPen(usize),
    PenUp(f64, f64),
    PenDownBegin,
    PenDownPoint(f64, f64),
    PenDownEnd,
    Complete,
}
```

## Summary

| Item | Detail |
|------|--------|
| New file | `hamego-core/src/async_parser.rs` |
| New trait | `AsyncCommandHandler` (6 async methods incl. `complete()`) |
| New function | `parse_hpgl_async<R: AsyncRead, H: AsyncCommandHandler>()` |
| Termination | `0x0A` byte → `handler.complete().await` |
| Buffer | Fixed-size stack `[u8; 256]`, no alloc |
| Dependency | `embedded-io-async` (optional, feature-gated) |
| Compatibility | `no_std`, no `alloc` |

