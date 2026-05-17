# Refactor: Split `parse_hpgl_async` into `StateMachine` struct

## Problem
`parse_hpgl_async` is ~250 lines — single loop with nested `match state` on 5 variants. All logic (IO, state machine, coordinate scaling, flush on DELIM/EOF/`;`) lives in one function body.

## Goal
Extract state into `StateMachine` struct with async methods, reducing `parse_hpgl_async` to a ~20-line IO loop.

---

## Steps

### 1. Create `StateMachine` struct
Move all mutable state variables into one struct:

```rust
struct StateMachine<'c> {
    state: State,
    prefix: Prefix,
    token: Token,
    carry: Option<(f64, f64)>,
    pu_x: Option<f64>,
    pd_x: Option<f64>,
    pt_count: usize,
    pending: bool,
    config: &'c Config,
}
```

Add `fn new(config: &Config) -> Self`.

### 2. Extract `flush_delim<H>(&mut self, handler: &mut H)`
Move the DELIM handling block (flush current state + reset + `handler.complete().await`).

### 3. Extract `flush_eof<H>(&mut self, handler: &mut H)`
Move the EOF block (flush trailing state + conditional `handler.complete().await`).

### 4. Extract `feed_byte<H, MAX_PTS, DELIM>(&mut self, b: u8, handler: &mut H) -> Result<(), ParseErrorKind>`
Main dispatch: DELIM check → `flush_delim`, otherwise `match self.state` dispatching to per-state handlers.

### 5. Extract per-state handlers (private async methods)
- `handle_command_byte(b, handler)` — prefix accumulation, transition to PD/PU/SP/Skip
- `handle_pd_byte(b, handler)` — PD coordinate parsing + streaming points
- `handle_pu_byte(b, handler)` — PU coordinate parsing, carry update
- `handle_sp_byte(b, handler)` — SP pen number parsing

### 6. Simplify `parse_hpgl_async`
After refactor, the public function becomes:

```rust
pub async fn parse_hpgl_async<const MAX_PTS: usize, const DELIM: u8, R, H>(
    mut reader: R,
    config: &Config,
    handler: &mut H,
    io_buf: &mut [u8],
) -> Result<(), ParseError<R::Error>>
where
    R: Read,
    H: AsyncCommandHandler,
{
    let mut sm = StateMachine::new(config);
    loop {
        let n = match reader.read(io_buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(ParseError::Io(e)),
        };
        for &b in &io_buf[..n] {
            sm.feed_byte::<MAX_PTS, DELIM, H>(b, handler).await
                .map_err(ParseError::from_kind)?;
        }
    }
    sm.flush_eof(handler).await;
    Ok(())
}
```

### 7. Verify
```sh
cargo fmt
cargo clippy
cargo test -p hamego-core --features async
```

---

## What NOT to change
- `Prefix`, `Token`, `State`, `scale_xy` — already separate, leave as-is
- Public API (`parse_hpgl_async` signature, `AsyncCommandHandler`, `ParseError`) — unchanged
- No `Action` enum — keep it simple with async `feed_byte` taking `&mut handler`

## Error handling note
`ParseErrorKind` (without `Io`) for internal methods; `parse_hpgl_async` wraps IO errors at the read site. Alternatively, keep `ParseError<core::convert::Infallible>` internally and convert.

