# Plan 07: Cursor-based parsing for coordinate handlers

## Problem
`handle_pd_byte`, `handle_pu_byte`, `handle_sp_byte` accumulate bytes one-by-one into `Token`, then parse on `,` or `;`. This creates branching complexity with `pending_x` state. The logic of "parse x, then y" is spread across multiple calls.

## Goal
Replace byte-by-byte token accumulation with a **cursor over an immutable buffer slice**. Each handler receives a `&[u8]` buffer and a `&mut usize` cursor. The handler tries to parse a complete value (or `(x, y)` pair) from the buffer starting at the cursor. On success it advances the cursor past the consumed bytes. On incomplete data it returns `None` — the caller retains unconsumed bytes for the next IO read.

## Design

### New internal parser: `Cursor`
A lightweight struct wrapping `(&[u8], usize)`:

```rust
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}
```

Methods:
- `peek(&self) -> Option<u8>` — next byte without advancing
- `advance(&mut self)` — move pos forward by 1
- `try_parse_f64(&mut self) -> Option<f64>` — scan digits/dot/sign until `,` or `;` or end, parse, advance past the number
- `try_parse_usize(&mut self) -> Option<usize>` — same for usize
- `try_parse_pair(&mut self) -> Option<(f64, f64)>` — parse `x,y` consuming both numbers and the separating `,`
- `skip_to(&mut self, b: u8)` — advance past next occurrence of `b`
- `at_end(&self) -> bool`
- `remaining(&self) -> &[u8]` — unconsumed slice

### How `feed_bytes` works (replaces `feed_byte`)

Instead of feeding one byte at a time, `feed_bytes` receives the entire `&io_buf[..n]` chunk. It creates a `Cursor` and loops:

```rust
async fn feed_bytes<const DELIM: u8, H>(&mut self, data: &[u8], handler: &mut H) -> Result<(), InnerError> {
    let mut cur = Cursor::new(data);
    while !cur.at_end() {
        let b = cur.peek().unwrap();
        if b == DELIM {
            cur.advance();
            self.flush_delim(handler).await;
            continue;
        }
        match self.state {
            State::Command => self.handle_command(&mut cur, handler).await,
            State::PdCoords => self.handle_pd(&mut cur, handler).await?,
            State::PuCoords => self.handle_pu(&mut cur, handler).await?,
            State::SpBody => self.handle_sp(&mut cur, handler).await?,
            State::Skip => cur.skip_to(b';'),
        }
    }
    // Return number of unconsumed bytes (for carry-over between reads)
    Ok(())
}
```

### `handle_pd` / `handle_pu` — new signature

```rust
async fn handle_pd<H>(&mut self, cur: &mut Cursor<'_>, handler: &mut H) -> Result<(), InnerError>
```

Logic:
1. Try `cur.try_parse_pair()` → `Some((x, y))`:
   - Scale, emit `pen_down_point(sx, sy)`, loop back for more pairs
2. `None` (incomplete pair at end of chunk):
   - Save unconsumed bytes in a small carry-over buffer `self.carry_buf`
   - Return — next `feed_bytes` call prepends carry bytes
3. On `;` → emit `pen_down_end()`, transition to `State::Command`

### `handle_sp` — similar

Try `cur.try_parse_usize()` → `Some(pen)` → `select_pen`, transition. Incomplete → carry.

### Carry-over buffer

When a number straddles two IO reads, we need to keep the partial bytes:

```rust
/// Bytes left over from the previous IO read that couldn't form a complete token.
carry_buf: [u8; TOKEN_BUF_SIZE],
carry_len: usize,
```

At the start of `feed_bytes`, if `carry_len > 0`, prepend carry bytes to the new data (copy into a small stack buffer or process carry first).

### Removal of `Token` and `pending_x`

- `Token` struct — **removed** (replaced by `Cursor` + `carry_buf`)
- `pending_x: Option<f64>` — **removed** (`try_parse_pair` parses both x and y atomically)

## Steps

### 1. Implement `Cursor` struct with parsing methods
### 2. Add `carry_buf` / `carry_len` to `StateMachine`, remove `Token` and `pending_x`
### 3. Rewrite `handle_pd` to use `Cursor::try_parse_pair`
### 4. Rewrite `handle_pu` to use `Cursor::try_parse_pair`
### 5. Rewrite `handle_sp` to use `Cursor::try_parse_usize`
### 6. Rewrite `handle_command` to use `Cursor`
### 7. Replace `feed_byte` with `feed_bytes` (chunk-level processing)
### 8. Update `parse_hpgl_async` to call `feed_bytes` instead of per-byte loop
### 9. Update `flush_delim` / `flush_eof` for new fields
### 10. Verify

```sh
cargo fmt
cargo clippy --workspace
cargo test -p hamego-core
cargo test --workspace
```

## Risks / Considerations

- **Carry-over complexity**: when a number like `"123"` is split across two reads (`"12"` + `"3,"`), the carry buffer must be handled correctly. Keep `TOKEN_BUF_SIZE` (16) as carry buffer size.
- **Atomicity of pair parsing**: `try_parse_pair` must not advance the cursor if only `x` is available but `y` is missing (incomplete at chunk boundary). Use a "checkpoint" pattern: save pos, try parse, restore on failure.
- **DELIM inside coordinates**: DELIM can appear mid-coordinate (e.g. `PD100,200,300\n`). The current code handles this in `flush_delim` — same logic applies but now cursor-based.

---

## Post-Implementation: Lessons Learned & Resolutions

### Problem 1: Infinite loop in `handle_pd` on orphan numbers

**Symptom**: `async_delim_inside_pd_coords` hung forever.

**Root cause**: `try_parse_pair` returned `PairResult::None` (e.g. single unpaired number `300` before DELIM) but the **cursor stayed in place**. The calling loop re-entered with the same state → infinite loop.

**Resolution**: When `PairResult::None` is returned, the handler explicitly skips bytes to the next `;` or DELIM to prevent re-entry without progress.

### Problem 2: `combined` cursor position accounting was wrong

**Symptom**: `async_pd_large_streams_correctly` only parsed 7–9 of 500 points.

**Root cause**: The initial design had handlers compute `consumed_in_cur = combined.pos - carry_len` to translate a position in the concatenated `carry + chunk` buffer back into the real `cur` cursor. This calculation broke in practice because:
1. The trailing inter-pair comma could or could not be consumed by `try_parse_pair`, creating inconsistency.
2. Whether `Incomplete` rewinds to x-start or after-comma position determined what bytes ended up in carry, and carry-comma logic kept getting out of sync.

**Resolution**: **Move carry prepend to `feed_bytes`** (the central dispatch), not inside individual handlers. `feed_bytes` builds a single `effective_slice = carry + new_data` on a stack buffer and passes a `Cursor` over it to handlers. Handlers become trivially simple — no combined cursor, no `consumed_in_cur` math.

### Problem 3: `try_parse_pair` Incomplete rewind point

**Symptom**: After switching to prepend-in-feed_bytes, some pairs were still lost.

**Root cause**: When y is incomplete, the cursor was rewound to `checkpoint_after_comma - 1` (keeping comma in remaining → into carry). The next chunk then starts with a raw comma in the effective slice, which `try_parse_pair` treats as an empty token → `None`.

**Resolution**: On **any** Incomplete (x or y), rewind cursor to `checkpoint` (the very start of x). Carry then contains `"x,y_partial"` or just `"x_partial"`. Next chunk will see `"x,y_rest,next..."` — `try_parse_pair` handles this cleanly since it parses x from the start.

### Problem 4: Double inter-pair commas

**Symptom**: `async_pd_large_streams_correctly` — 9/500 points.

**Root cause**: The test generates `PD100,200,,100,200,,100,200,...;` (two commas between pairs due to a loop that pushes comma both after and before). A single `if peek == ','` only skips one.

**Resolution**: Skip **all** leading commas with `while cur.peek() == Some(b',') { cur.advance(); }` before attempting to parse a pair.

### Problem 5: `TokenTooLong` detection

**Symptom**: `async_token_too_long_returns_err` failed (returned `Ok` instead of `Err`).

**Root cause**: In the cursor-based approach, numbers are parsed by scanning until a delimiter. Without an explicit length check, any length passes. `f64::parse` happily parses very long digit strings.

**Resolution**: Added `NumberResult::TooLong` variant. `try_parse_number` returns `TooLong` if the scanned token exceeds `CARRY_BUF_SIZE` bytes. Handlers propagate it as `InnerError::TokenTooLong`.

---

## Final Architecture (implemented)

```
parse_hpgl_async
  └── loop { reader.read(io_buf) }
        └── feed_bytes(data)
              1. Prepend carry_buf to data → effective_slice (stack buffer, ≤ 16+256+256 bytes)
              2. Create Cursor over effective_slice
              3. Loop: DELIM? → flush_delim. Otherwise dispatch by state:
                   Command → handle_command (reads 2-byte prefix)
                   PdCoords → handle_pd (loop: skip commas, parse pairs, emit points)
                   PuCoords → handle_pu (same, but stores last pair in pu_carry)
                   SpBody → handle_sp (parse usize pen number)
                   Skip → advance to ';'
              4. On Incomplete: carry_push(remaining), exit feed_bytes
  └── flush_eof (emit pen_down_end + complete if pending)
```

## Files modified
- `hamego-core/src/async_parser.rs` — complete rewrite per above
- No test changes needed — all 16 tests pass

