# Plan 04: External Buffer + Embassy Migration

## Goal

1. Remove stack-allocated I/O buffer from `parse_hpgl_async` — accept it as a `&mut [u8]` parameter instead.
2. Enable callers to pass a reference to a statically-allocated buffer (e.g. behind an `embassy_sync::Mutex`).
3. Replace `tokio` with `embassy` in the CLI crate.

## Motivation

- On embedded targets stack space is precious; a `static` buffer avoids blowing the stack.
- `embassy` is the de-facto async runtime for `no_std` Rust — aligns with the `no_std` core crate.
- Passing `&mut [u8]` makes the buffer size a runtime value — removes the `const BS` generic, simplifying the signature.

## Changes

### 1. `hamego-core/src/async_parser.rs`

**Remove** `const BS: usize` generic parameter.

**New signature:**
```rust
pub async fn parse_hpgl_async<const MAX_PTS: usize, const DELIM: u8, R, H>(
    mut reader: R,
    config: &Config,
    handler: &mut H,
    io_buf: &mut [u8],       // ← externally-owned buffer
) -> Result<(), ParseError<R::Error>>
where
    R: Read,
    H: AsyncCommandHandler,
```

- `io_buf.len()` determines chunk size at runtime.
- `DEFAULT_IO_BUF_SIZE` constant stays as documentation/recommendation but is no longer enforced by generics.
- Internal `token` buffer (`[u8; TOKEN_BUF_SIZE]`) remains stack-allocated — it's only 16 bytes.

### 2. `hamego-core/Cargo.toml`

No changes — `embedded-io-async` remains the only async dependency.

### 3. `hamego` CLI crate — Embassy migration

**Replace** `tokio` with `embassy-executor` (std feature for desktop).

**New dependencies:**
```toml
[dependencies]
embassy-executor = { version = "0.7", features = ["arch-std", "executor-thread"] }
embassy-sync = { version = "0.6" }
embedded-io-async = "0.6"
critical-section = { version = "1", features = ["std"] }
```

**Approach:** Use `embassy-executor` with `arch-std` + `executor-thread` features. Wrap `std::io::Read` into `embedded_io_async::Read` via a trivial `StdFileReader` adapter (blocking reads are acceptable on a desktop single-thread executor).

**`src/main.rs` structure:**
```rust
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

static IO_BUFFER: Mutex<CriticalSectionRawMutex, [u8; 4096]> = Mutex::new([0u8; 4096]);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // parse CLI args
    // open file
    let mut buf = IO_BUFFER.lock().await;
    // call parse_hpgl_async with &mut *buf
    parse_hpgl_async::<4096, 0x0A, _, _>(reader, &config, &mut handler, &mut *buf).await.unwrap();
}
```

The buffer is always behind `Mutex<CriticalSectionRawMutex, [u8; N]>` — same pattern on desktop and embedded, no exceptions.

### 4. Remove `tokio` dependency

- `Cargo.toml`: remove `tokio`, add `embassy-sync` with `critical-section` support
- `src/main.rs`: replace `#[tokio::main]` with `#[embassy_executor::main]`
- `StdFileReader`: simple wrapper implementing `embedded_io_async::Read` over `std::io::Read` (blocking)

### 5. Update tests

- All test helpers pass `&mut [0u8; DEFAULT_IO_BUF_SIZE]` as the buffer argument.
- Parity test uses `&mut [0u8; 4096]` (larger for test5.hpgl performance).
- `futures-lite` remains for `block_on` in tests (embassy executor not needed in test harness).

### 6. Update `hamego-wasm`

- Pass a `&mut [u8]` from the wasm module's stack or a `static mut` if needed.
- No runtime change (wasm doesn't use embassy — just calls the async fn via `wasm_bindgen_futures`).

## Summary of generic parameter changes

| Before | After |
|--------|-------|
| `<const BS, const MAX_PTS, const DELIM, R, H>` | `<const MAX_PTS, const DELIM, R, H>` + `io_buf: &mut [u8]` |

## File changes overview

| File | Action |
|------|--------|
| `hamego-core/src/async_parser.rs` | Remove `BS` generic, add `io_buf` param |
| `hamego-core/tests/async_parser_tests.rs` | Pass buffer to calls |
| `hamego/Cargo.toml` | Remove `tokio`, add `embassy-executor`, `embassy-sync`, `embedded-io-adapters` |
| `hamego/src/main.rs` | `#[embassy_executor::main]`, `StdFileReader`, pass `&mut buf` |
| `hamego-wasm/src/lib.rs` | Pass `&mut buf` to `parse_hpgl_async` (if used) |

## Tests to write/update

| # | Test | What it verifies |
|---|------|-----------------|
| 1 | `async_parses_single_command` | Basic SP with external buffer |
| 2 | `async_complete_fires_on_lf` | DELIM detection unchanged |
| 3 | `async_no_complete_on_eof` | EOF behaviour unchanged |
| 4 | `async_continues_after_hpgl_block_ends` | Multi-transmission |
| 5 | `async_empty_pu_is_skipped` | Empty PU |
| 6 | `async_parses_pen_up_pen_down_sequence` | Coordinate scaling + carry |
| 7 | `async_parity_with_sync_parser` | Full file parity |
| 8 | `async_command_too_long_returns_err` | TokenTooLong error |
| 9 | `async_buffer_overflow_handled` | TokenTooLong on large input |
| 10 | `async_too_many_points` | TooManyPoints error |
| 11 | `async_small_io_buffer` | Works with `&mut [0u8; 4]` — very small chunks |
| 12 | `async_empty_pd_with_carry` | PU→PD; emits carry only |
| 13 | `async_unknown_command_skipped` | IN; ignored |
| 14 | `async_pu_multiple_pairs` | Only last PU pair becomes carry |
| 15 | `async_multi_digit_sp` | SP12 parses correctly |
| 16 | `async_pd_too_many_points` | Returns TooManyPoints |
| 17 | `async_delim_inside_pd_coords` | DELIM mid-PD flushes + complete |

All existing 17 tests adapted — only change is adding the buffer argument.

