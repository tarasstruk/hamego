# Plan: Streaming PD Coordinate Parser (No Large Buffer)

## Problem

`DEFAULT_CMD_BUF_SIZE = 16384` — a 16 KB stack allocation per call — is excessive.
The root cause: `PD` commands in real HPGL files can contain hundreds of coordinate
pairs in one semicolon-delimited token (up to ~10 KB in `test5.hpgl`).
Buffering the entire `PD` body before dispatch defeats the purpose of streaming.

## Goal

Parse `PD` coordinate pairs **on-the-fly** as bytes arrive, without ever buffering
more than one number token at a time.

---

## Design

### Key insight

The only reason we need a large buffer is to hold the full `PD...;` body before
calling `read_points()`. If we detect the `PD` prefix early and switch into a
**coordinate streaming mode**, we can emit `pen_down_point()` events as each
`x,y` pair is completed — with only a tiny token buffer (e.g. 16 bytes per number).

### Parser state machine

Replace the flat byte-accumulation loop with an explicit state machine:

```rust
enum ParseState {
    /// Accumulating command prefix bytes (max 2: "SP", "PU", "PD", etc.)
    Command,
    /// Inside SP body — accumulating pen number digits into token_buf
    SpBody,
    /// Inside PU body — streaming coordinate pairs, keeping only the last one
    PuCoords,
    /// Inside PD body — streaming coordinate pairs as pen_down_point() events
    PdCoords,
    /// Unknown command — discard bytes until ';' or DELIM
    SkipToSemicolon,
}
```

Shared coordinate parsing state (used by both `PuCoords` and `PdCoords`):
```rust
token_buf: [u8; TOKEN_BUF_SIZE],  // current number being accumulated
token_len: usize,
pending_x: Option<f64>,           // x of current pair (waiting for y)
point_count: usize,               // points emitted in current PD
```

### State transitions

```
Start → Command

Command + byte (accumulate prefix, max 2 bytes):
  prefix == "PD" → emit carry-over point if any, enter PdCoords
  prefix == "PU" → enter PuCoords
  prefix == "SP" → enter SpBody
  prefix == other 2 chars → enter SkipToSemicolon
  prefix + ';' (before 2 chars) → stay Command (empty/1-char command, ignore)
  prefix + DELIM → flush + complete() + reset to Command

PdCoords + digit → accumulate in token_buf
PdCoords + ',' →
  if pending_x is None → parse token as x, store in pending_x
  if pending_x is Some → parse token as y, apply scale, emit pen_down_point(x,y), increment point_count
    if point_count > MAX_PTS → return Err(TooManyPoints)
PdCoords + ';' → flush partial pair (ignored — unpaired x is dropped), pen_down_end(), → Command
PdCoords + DELIM → pen_down_end() + complete() + reset to Command

PuCoords + digit → accumulate in token_buf
PuCoords + ',' →
  if pending_x is None → parse token as x
  if pending_x is Some → parse as y, apply scale, overwrite current_point
PuCoords + ';' → emit pen_up(last_point) if valid, → Command
PuCoords + DELIM → emit pen_up(last_point) + complete() + reset to Command

SpBody + digit → accumulate in token_buf
SpBody + ';' → parse pen number, call select_pen(), → Command
SpBody + DELIM → parse pen number, call select_pen(), complete(), → Command

SkipToSemicolon + ';' → Command
SkipToSemicolon + DELIM → complete() + Command
SkipToSemicolon + byte → discard
```

### Token buffer size

One coordinate number is at most 4 digits (HPGL max 9999):
```rust
const TOKEN_BUF_SIZE: usize = 16; // generous for decimals/negatives
```

### PU coordinate handling

`PU` only needs the **last** coordinate pair (to set `current`).
Stream and overwrite — keep only the last seen pair, no buffer needed beyond token.

### Unknown commands

Any 2-char prefix not matching SP/PU/PD → enter `SkipToSemicolon` state, discard bytes until `;`.

### Edge cases

- **Unpaired coordinate at `;`**: if `pending_x` has a value but no `y` arrived before `;`, drop silently. This handles malformed input gracefully.
- **Empty PD body** (`PD;`): `pen_down_begin()` + carry-over point (if any) + `pen_down_end()` — emits a single-point path or empty path.
- **DELIM inside coords**: always flush current state properly before calling `complete()`.

---

## New constants

```rust
pub const DEFAULT_IO_BUF_SIZE: usize = 256;  // I/O read chunk size
pub const TOKEN_BUF_SIZE: usize = 16;         // one coordinate number
pub const DEFAULT_MAX_PTS: usize = 4096;      // max points per PD path
pub const DEFAULT_DELIM: u8 = 0x0A;           // LF (already exists)
```

Total stack usage per `parse_hpgl_async` call: `BS (I/O) + 16 (token) + 2 (prefix)` ≈ `BS + 18` bytes + a few f64s.
With `BS = 256`: **~300 bytes** vs current **32 KB**.

---

## Steps

### 1. Define `ParseState` enum in `async_parser.rs`

### 2. Rewrite the byte-processing loop

Replace the current flat accumulator with a `match` on `ParseState`.
Each incoming byte is handled according to current state.

### 3. Token buffer

Small fixed `[u8; TOKEN_BUF_SIZE]` + `token_len: usize` replaces the large `cmd_buf`.
Parse `f64` from token on `,` or `;`.

### 4. Rename `DEFAULT_CMD_BUF_SIZE` → `DEFAULT_IO_BUF_SIZE`

`BS` now controls only the **I/O read chunk size**, not command buffering.

### 5. Add `MAX_PTS` generic parameter

```rust
pub async fn parse_hpgl_async<
    const BS: usize,       // I/O read chunk size
    const MAX_PTS: usize,  // max coordinate pairs per PD path
    const DELIM: u8,       // transmission delimiter
    R: Read,
    H: AsyncCommandHandler,
>(...)
```

New `ParseError` variant:
```rust
pub enum ParseError<E> {
    TooManyPoints,   // a single PD path exceeded MAX_PTS coordinate pairs
    TokenTooLong,    // a number token exceeded TOKEN_BUF_SIZE
    Io(E),
}
```

### 6. Remove `dispatch()` and `read_points()` usage

The state machine handles all dispatch inline. Coordinate scaling is done
in-place when emitting points: `(raw_x * scale, (height - raw_y) * scale)`.

### 7. Update tests

See Tests section below.

---

## Tests

All tests use `RecordingHandler` (already exists) or counters.

### Preserved tests (updated for new generic params)

| Test | What it verifies |
|------|-----------------|
| `async_parses_single_command` | `SP1;` + DELIM → SelectPen(1) + Complete |
| `async_complete_fires_on_lf` | DELIM triggers exactly one Complete |
| `async_no_complete_on_eof` | EOF without DELIM → no Complete |
| `async_continues_after_hpgl_block_ends` | DELIM resets state, parsing continues |
| `async_empty_pu_is_skipped` | `PU;` with no coords emits nothing |
| `async_parses_pen_up_pen_down_sequence` | PU→PD carry-over, coordinate scaling |
| `async_parity_with_sync_parser` | event counts match sync `parse_hpgl` on `test5.hpgl` |

### New tests

| Test | What it verifies |
|------|-----------------|
| `async_token_too_long_returns_err` | A number token > 16 chars → `Err(ParseError::TokenTooLong)` |
| `async_too_many_points_returns_err` | PD with `MAX_PTS + 1` pairs → `Err(ParseError::TooManyPoints)` |
| `async_pd_large_streams_correctly` | PD with 500 coordinate pairs (< MAX_PTS) → exactly 500 PenDownPoint events + Begin/End |
| `async_pd_empty_body` | `PD;` → PenDownBegin + PenDownEnd (no points, unless carry-over) |
| `async_pd_with_carry_and_empty_body` | `PU100,200;PD;` → PenDownBegin + PenDownPoint(carry) + PenDownEnd |
| `async_unknown_command_skipped` | `IN;LA1,2;PD100,200;` → only PD events emitted, IN/LA silently skipped |
| `async_delim_inside_pd_coords` | `PD100,200,300` + DELIM → PenDownPoint(100,200) + PenDownEnd + Complete (unpaired 300 dropped) |
| `async_pu_multiple_pairs_keeps_last` | `PU100,200,300,400;` → pen_up called with (300,400) only |
| `async_sp_multidigit` | `SP12;` → select_pen(12) (edge case for >1 digit pen) |
| `async_small_io_buf` | Full test5.hpgl with `BS=16` — produces same result as `BS=256` (verifies no I/O chunking bugs) |

---

## Summary

| | Before | After |
|---|---|---|
| Stack per call | `2 × BS` (up to 32 KB) | `BS + ~20` bytes |
| `PD` buffering | Full body in `cmd_buf` | None — streamed point-by-point |
| `CommandTooLong` error | Possible on large `PD` | Eliminated — replaced by `TokenTooLong` |
| Too-many-points protection | None | `Err(ParseError::TooManyPoints)` at `MAX_PTS` |
| Token buffer | None (full cmd buffer) | `[u8; 16]` |
| State machine | Implicit (prefix match) | Explicit `ParseState` enum |
| Generic params | `BS, DELIM` | `BS, MAX_PTS, DELIM` |

