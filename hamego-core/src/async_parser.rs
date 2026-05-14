use core::str::FromStr;

use embedded_io_async::Read;

use crate::Config;

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Default I/O read chunk size for [`parse_hpgl_async`].
pub const DEFAULT_IO_BUF_SIZE: usize = 256;

/// Default maximum coordinate pairs allowed in a single PD path.
/// `test5.hpgl` largest PD has ~640 pairs — 4096 is comfortable headroom.
pub const DEFAULT_MAX_PTS: usize = 4096;

/// Default delimiter byte — LF signals end of one HPGL transmission.
pub const DEFAULT_DELIM: u8 = 0x0A;

/// Internal token buffer capacity — fits any HPGL number (max 4 digits + sign/dot).
const TOKEN_BUF_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`parse_hpgl_async`].
#[derive(Debug, PartialEq)]
pub enum ParseError<E> {
    /// A single PD path contained more coordinate pairs than `MAX_PTS`.
    TooManyPoints,
    /// A number token exceeded [`TOKEN_BUF_SIZE`] bytes.
    TokenTooLong,
    /// The underlying reader returned an I/O error.
    Io(E),
}

// ---------------------------------------------------------------------------
// AsyncCommandHandler trait
// ---------------------------------------------------------------------------

/// Async streaming event handler for HPGL commands.
#[allow(async_fn_in_trait)]
pub trait AsyncCommandHandler {
    async fn select_pen(&mut self, pen: usize);
    async fn pen_up(&mut self, x: f64, y: f64);
    /// Called before the first point of a PD segment.
    async fn pen_down_begin(&mut self);
    /// Called for each point in the current PD segment (scaled SVG coordinates).
    async fn pen_down_point(&mut self, x: f64, y: f64);
    /// Called after the last point of a PD segment.
    async fn pen_down_end(&mut self);
    /// Called when `DELIM` byte is received — signals end of HPGL transmission.
    async fn complete(&mut self);
}

// ---------------------------------------------------------------------------
// Internal state machine
// ---------------------------------------------------------------------------

/// Two-byte command prefix accumulator.
#[derive(Default)]
struct Prefix {
    buf: [u8; 2],
    len: usize,
}

impl Prefix {
    fn push(&mut self, b: u8) {
        if self.len < 2 {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }
    fn is_full(&self) -> bool {
        self.len == 2
    }
    fn matches(&self, s: &[u8; 2]) -> bool {
        self.len == 2 && &self.buf == s
    }
    fn reset(&mut self) {
        self.len = 0;
    }
}

/// Token accumulator for a single number.
struct Token {
    buf: [u8; TOKEN_BUF_SIZE],
    len: usize,
}

impl Token {
    fn new() -> Self {
        Self {
            buf: [0; TOKEN_BUF_SIZE],
            len: 0,
        }
    }
    fn push(&mut self, b: u8) -> bool {
        if self.len >= TOKEN_BUF_SIZE {
            return false; // overflow
        }
        self.buf[self.len] = b;
        self.len += 1;
        true
    }
    fn parse_f64(&self) -> Option<f64> {
        let s = core::str::from_utf8(&self.buf[..self.len]).ok()?;
        f64::from_str(s.trim()).ok()
    }
    fn parse_usize(&self) -> Option<usize> {
        let s = core::str::from_utf8(&self.buf[..self.len]).ok()?;
        usize::from_str(s.trim()).ok()
    }
    fn reset(&mut self) {
        self.len = 0;
    }
}

/// Parser state.
enum State {
    /// Reading the 2-byte command prefix.
    Command,
    /// Inside `SP` body — reading pen number digits.
    SpBody,
    /// Inside `PU` body — streaming coordinate pairs, keeping only the last.
    PuCoords,
    /// Inside `PD` body — streaming coordinate pairs as events.
    PdCoords,
    /// Unknown command — discard until `;` or DELIM.
    Skip,
}

// ---------------------------------------------------------------------------
// Scale helper (inline, no heap)
// ---------------------------------------------------------------------------

#[inline]
fn scale_xy(raw_x: f64, raw_y: f64, config: &Config) -> (f64, f64) {
    (raw_x * config.scale, (config.height - raw_y) * config.scale)
}

// ---------------------------------------------------------------------------
// parse_hpgl_async
// ---------------------------------------------------------------------------

/// Parse HPGL from an async byte reader, streaming events to `handler`.
///
/// # Generic parameters
/// - `BS` — I/O read chunk size. Use [`DEFAULT_IO_BUF_SIZE`].
/// - `MAX_PTS` — maximum coordinate pairs per PD path. Returns
///   [`ParseError::TooManyPoints`] if exceeded. Use [`DEFAULT_MAX_PTS`].
/// - `DELIM` — byte that signals end of one transmission, causing
///   `handler.complete()` to be called. Use [`DEFAULT_DELIM`] (`0x0A`).
///
/// # Behaviour
/// - Commands are delimited by `;`.
/// - DELIM resets state and continues parsing (multiple transmissions supported).
/// - EOF without DELIM exits silently without calling `complete()`.
#[allow(unused_assignments)] // macro_rules! do_complete! resets carry/pt_count; compiler sees them as dead assignments
pub async fn parse_hpgl_async<const BS: usize, const MAX_PTS: usize, const DELIM: u8, R, H>(
    mut reader: R,
    config: &Config,
    handler: &mut H,
) -> Result<(), ParseError<R::Error>>
where
    R: Read,
    H: AsyncCommandHandler,
{
    let mut io_buf = [0u8; BS];
    let mut state = State::Command;
    let mut prefix = Prefix::default();
    let mut token = Token::new();
    // Carry-over point from the last PU (scaled coords)
    let mut carry: Option<(f64, f64)> = None;
    // For PuCoords: last seen x (unscaled), to overwrite on each new pair
    let mut pu_x: Option<f64> = None;
    // For PdCoords: x of current pair (waiting for y)
    let mut pd_x: Option<f64> = None;
    // Points emitted in the current PD path
    let mut pt_count: usize = 0;

    // Flush helpers as closures are not async — use a macro for the DELIM path
    macro_rules! do_complete {
        () => {
            pu_x = None;
            pd_x = None;
            pt_count = 0;
            token.reset();
            prefix.reset();
            carry = None;
            state = State::Command;
            handler.complete().await;
        };
    }

    loop {
        let n = match reader.read(&mut io_buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(ParseError::Io(e)),
        };

        for &b in &io_buf[..n] {
            // ----------------------------------------------------------------
            // DELIM — flush current state, fire complete(), reset
            // ----------------------------------------------------------------
            if b == DELIM {
                match state {
                    State::PdCoords => {
                        // flush final pair if complete, drop unpaired x
                        if let (Some(rx), Some(ry)) = (pd_x.take(), token.parse_f64()) {
                            let (sx, sy) = scale_xy(rx, ry, config);
                            if pt_count < MAX_PTS {
                                handler.pen_down_point(sx, sy).await;
                            }
                        }
                        handler.pen_down_end().await;
                    }
                    State::PuCoords => {
                        // flush last seen pu pair
                        if let (Some(x), Some(y)) = (pu_x.take(), token.parse_f64()) {
                            let (sx, sy) = scale_xy(x, y, config);
                            carry = Some((sx, sy));
                            handler.pen_up(sx, sy).await;
                        }
                        token.reset();
                    }
                    State::SpBody => {
                        if let Some(pen) = token.parse_usize() {
                            handler.select_pen(pen).await;
                        }
                        token.reset();
                    }
                    _ => {}
                }
                do_complete!();
                continue;
            }

            // ----------------------------------------------------------------
            // State machine
            // ----------------------------------------------------------------
            match state {
                // ------------------------------------------------------------
                State::Command => {
                    if b == b';' {
                        // Empty or single-char command — ignore
                        prefix.reset();
                        continue;
                    }
                    prefix.push(b);
                    if prefix.is_full() {
                        if prefix.matches(b"PD") {
                            handler.pen_down_begin().await;
                            // Emit carry-over point from last PU
                            if let Some((cx, cy)) = carry.take() {
                                handler.pen_down_point(cx, cy).await;
                                pt_count = 1;
                            } else {
                                pt_count = 0;
                            }
                            pd_x = None;
                            token.reset();
                            state = State::PdCoords;
                        } else if prefix.matches(b"PU") {
                            pu_x = None;
                            pd_x = None;
                            token.reset();
                            state = State::PuCoords;
                        } else if prefix.matches(b"SP") {
                            token.reset();
                            state = State::SpBody;
                        } else {
                            state = State::Skip;
                        }
                        prefix.reset();
                    }
                }

                // ------------------------------------------------------------
                State::PdCoords => {
                    if b == b';' {
                        // Flush final pair if complete (x already stored, token has y)
                        if let (Some(rx), Some(ry)) = (pd_x.take(), token.parse_f64()) {
                            let (sx, sy) = scale_xy(rx, ry, config);
                            if pt_count >= MAX_PTS {
                                return Err(ParseError::TooManyPoints);
                            }
                            handler.pen_down_point(sx, sy).await;
                            pt_count += 1;
                        }
                        // Drop unpaired x (odd coordinate count — malformed input)
                        pd_x = None;
                        token.reset();
                        pt_count = 0;
                        handler.pen_down_end().await;
                        state = State::Command;
                    } else if b == b',' {
                        if pd_x.is_none() {
                            // First of pair: store x
                            pd_x = token.parse_f64();
                            token.reset();
                        } else {
                            // Second of pair: emit point
                            if let (Some(rx), Some(ry)) = (pd_x.take(), token.parse_f64()) {
                                let (sx, sy) = scale_xy(rx, ry, config);
                                if pt_count >= MAX_PTS {
                                    return Err(ParseError::TooManyPoints);
                                }
                                handler.pen_down_point(sx, sy).await;
                                pt_count += 1;
                            }
                            token.reset();
                        }
                    } else {
                        if !token.push(b) {
                            return Err(ParseError::TokenTooLong);
                        }
                    }
                }

                // ------------------------------------------------------------
                State::PuCoords => {
                    if b == b';' {
                        // Flush last pair: pu_x already set, token has y
                        if let (Some(rx), Some(ry)) = (pu_x.take(), token.parse_f64()) {
                            let (sx, sy) = scale_xy(rx, ry, config);
                            carry = Some((sx, sy));
                            handler.pen_up(sx, sy).await;
                        }
                        token.reset();
                        state = State::Command;
                    } else if b == b',' {
                        if pu_x.is_none() {
                            // First of pair: x
                            pu_x = token.parse_f64();
                            token.reset();
                        } else {
                            // Second of pair: overwrite carry with new y
                            if let (Some(rx), Some(ry)) = (pu_x.take(), token.parse_f64()) {
                                let (sx, sy) = scale_xy(rx, ry, config);
                                carry = Some((sx, sy));
                                handler.pen_up(sx, sy).await;
                            }
                            token.reset();
                            // Stay in PuCoords — more pairs may follow
                        }
                    } else {
                        if !token.push(b) {
                            return Err(ParseError::TokenTooLong);
                        }
                    }
                }

                // ------------------------------------------------------------
                State::SpBody => {
                    if b == b';' {
                        if let Some(pen) = token.parse_usize() {
                            handler.select_pen(pen).await;
                        }
                        token.reset();
                        state = State::Command;
                    } else {
                        if !token.push(b) {
                            return Err(ParseError::TokenTooLong);
                        }
                    }
                }

                // ------------------------------------------------------------
                State::Skip => {
                    if b == b';' {
                        state = State::Command;
                    }
                    // else: discard
                }
            }
        }
    }

    // EOF: flush trailing state without complete()
    match state {
        State::PdCoords => {
            handler.pen_down_end().await;
        }
        State::PuCoords => {
            if let (Some(rx), Some(ry)) = (pu_x, token.parse_f64()) {
                let (sx, sy) = scale_xy(rx, ry, config);
                handler.pen_up(sx, sy).await;
            }
        }
        State::SpBody => {
            if let Some(pen) = token.parse_usize() {
                handler.select_pen(pen).await;
            }
        }
        _ => {}
    }

    Ok(())
}
